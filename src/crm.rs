// SPDX-License-Identifier: GPL-3.0-only

//! Notes, interactions, and keeping in touch.
//!
//! The layer between an address book and a CRM (03 §7): what you last said to
//! someone, when, and how often you meant to. None of it is vCard data and
//! none of it belongs on a card other people sync — a shared work address book
//! must not grow a field saying when you last rang your colleague. So it lives
//! beside the books rather than inside them, in `$contacts_root/.crm/`, a
//! dot-directory the vdir collection scanner skips and sync therefore never
//! provisions or pushes.
//!
//! # Why records are keyed by card, not by person
//!
//! A linked person is several cards ([`crate::links`]), and CRM data could
//! reasonably attach to either. Keying by **card** is what makes linking and
//! unlinking lossless in both directions: a person's notes are the union of
//! their cards' notes, exactly as the detail view unions their addresses, and
//! taking a card back out takes its own notes with it. Keying by person would
//! need a migration every time two people became one.
//!
//! # Favorites
//!
//! A star is the same kind of fact as a note: yours, about somebody, and
//! nobody else's business. It is kept here and never written into the card,
//! so it stays on this computer — it does not sync, and a shared address
//! book does not tell its other readers whom you starred.
//!
//! Starring a person stars every one of their cards, and a person is starred
//! while any of their cards is. That is what keeps the star where it was put
//! through linking, unlinking, and deleting one card of several.
//!
//! # What is deliberately not here
//!
//! No scheduler and no notifications. "Overdue" is a question asked of the
//! data when the list is drawn, not a timer — 03 §7 is explicit that reminders
//! wait for Slate's machinery to move to the substrate rather than growing a
//! second one here.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Utc};

use cosmic_pim_core::model::Contact;

use crate::links::{CardRef, LinkStore};

/// One timestamped note.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Note {
    /// Stable across edits and reorderings, so a delete names one note rather
    /// than a position in a list that may have shifted underneath it.
    pub id: String,
    pub at: DateTime<Utc>,
    pub text: String,
}

/// One logged contact with a person.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Interaction {
    pub at: DateTime<Utc>,
    /// Free text — "called", "coffee", "replied to their mail". Not an enum:
    /// the useful vocabulary here is the user's, and a fixed list would be
    /// wrong for most people most of the time.
    #[serde(default)]
    pub kind: String,
}

/// Everything recorded against one card.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct Record {
    #[serde(default)]
    pub notes: Vec<Note>,
    #[serde(default)]
    pub interactions: Vec<Interaction>,
    /// How often you meant to be in touch. `None` means you never said.
    #[serde(default)]
    pub cadence_days: Option<u32>,
    /// Files kept with this card — see [`crate::attachments`]. The bytes live
    /// in the shared blob directory; these are references to them.
    #[serde(default)]
    pub attachments: Vec<crate::attachments::Attachment>,
    /// Starred — see the module docs.
    #[serde(default)]
    pub favorite: bool,
}

impl Record {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty()
            && self.interactions.is_empty()
            && self.cadence_days.is_none()
            && self.attachments.is_empty()
            && !self.favorite
    }

    #[must_use]
    pub fn last_contacted(&self) -> Option<DateTime<Utc>> {
        self.interactions.iter().map(|i| i.at).max()
    }
}

/// The CRM store: one JSON file per card, under `.crm/`.
pub struct CrmStore {
    dir: PathBuf,
    records: HashMap<CardRef, Record>,
    /// Record files the last reload could not read. Their contents — and the
    /// attachments they name — are unknown, which matters to anything that
    /// decides by absence; see [`Self::read_every_record`].
    unreadable: usize,
}

impl CrmStore {
    /// Opens the store under the given contacts root, creating nothing until
    /// the first write.
    #[must_use]
    pub fn open(contacts_root: &Path) -> Self {
        let mut store = Self {
            dir: contacts_root.join(".crm"),
            records: HashMap::new(),
            unreadable: 0,
        };
        store.reload();
        store
    }

    /// Re-reads every record from disk.
    pub fn reload(&mut self) {
        self.records.clear();
        self.unreadable = 0;
        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return; // No directory yet: nothing recorded, which is fine.
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let Some(card) = decode_key(stem) else {
                tracing::warn!(?path, "skipping a CRM record with an unreadable name");
                self.unreadable += 1;
                continue;
            };
            match std::fs::read_to_string(&path).map(|t| serde_json::from_str::<Record>(&t)) {
                Ok(Ok(record)) => {
                    self.records.insert(card, record);
                }
                Ok(Err(why)) => {
                    tracing::warn!(?path, %why, "skipping an unreadable CRM record");
                    self.unreadable += 1;
                }
                Err(why) => {
                    tracing::warn!(?path, %why, "skipping an unreadable CRM record");
                    self.unreadable += 1;
                }
            }
        }
    }

    /// Whether the last reload read every record file there was.
    ///
    /// `false` means some record exists that this store knows nothing about,
    /// so a question answered by absence — "does anything still name this
    /// blob?" — cannot be answered safely.
    #[must_use]
    pub fn read_every_record(&self) -> bool {
        self.unreadable == 0
    }

    #[must_use]
    pub fn record(&self, card: &CardRef) -> Option<&Record> {
        self.records.get(card)
    }

    /// Whether anything at all has been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.values().all(Record::is_empty)
    }

    /// Whether anybody has a cadence — what the "Keep in touch" sidebar entry
    /// is hidden on.
    ///
    /// Not [`Self::is_empty`]: that list holds only people past a cadence, so
    /// attaching a file or writing a note would otherwise conjure a sidebar
    /// entry that can never have a row in it.
    #[must_use]
    pub fn has_any_cadence(&self) -> bool {
        self.records.values().any(|r| r.cadence_days.is_some())
    }

    /// Every blob any record names — the reference set the attachment sweep
    /// works from.
    #[must_use]
    pub fn referenced_blobs(&self) -> std::collections::HashSet<String> {
        self.records
            .values()
            .flat_map(|record| record.attachments.iter().map(|a| a.blob.clone()))
            .collect()
    }

    /// Adds a note against one card.
    pub fn add_note(
        &mut self,
        card: &CardRef,
        text: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        let text = text.trim();
        if text.is_empty() {
            return Ok(()); // An empty note is not a note.
        }
        self.change(card, |record| {
            record.notes.push(Note {
                id: uuid::Uuid::new_v4().simple().to_string(),
                at: now,
                text: text.to_owned(),
            });
        })
    }

    /// Removes one note by id. Removing the last thing on a record deletes the
    /// file rather than leaving an empty one behind.
    pub fn remove_note(&mut self, card: &CardRef, id: &str) -> Result<(), String> {
        if !self.records.contains_key(card) {
            return Ok(());
        }
        self.change(card, |record| record.notes.retain(|note| note.id != id))
    }

    /// Records that you were in touch.
    pub fn log_interaction(
        &mut self,
        card: &CardRef,
        kind: &str,
        now: DateTime<Utc>,
    ) -> Result<(), String> {
        self.change(card, |record| {
            record.interactions.push(Interaction {
                at: now,
                kind: kind.trim().to_owned(),
            });
        })
    }

    /// Sets, or clears, how often you mean to be in touch with this card.
    pub fn set_cadence(&mut self, card: &CardRef, days: Option<u32>) -> Result<(), String> {
        self.change(card, |record| record.cadence_days = days.filter(|d| *d > 0))
    }

    /// Attaches an already-stored file to a card.
    ///
    /// Attaching the same blob twice is a no-op rather than a duplicate row:
    /// the digest is the identity, so there is nothing a second copy could
    /// mean.
    pub fn attach(
        &mut self,
        card: &CardRef,
        attachment: crate::attachments::Attachment,
    ) -> Result<(), String> {
        if self
            .records
            .get(card)
            .is_some_and(|r| r.attachments.iter().any(|a| a.blob == attachment.blob))
        {
            return Ok(());
        }
        self.change(card, |record| record.attachments.push(attachment))
    }

    /// Removes one attachment reference. The blob itself is the caller's
    /// business — see [`Self::is_blob_referenced`].
    pub fn detach(&mut self, card: &CardRef, blob: &str) -> Result<(), String> {
        if !self.records.contains_key(card) {
            return Ok(());
        }
        self.change(card, |record| record.attachments.retain(|a| a.blob != blob))
    }

    /// Whether any record still names this blob.
    ///
    /// The records are the reference count. A stored counter would be one
    /// more thing to keep in sync, and when it drifted it would either orphan
    /// a file forever or delete one somebody was still using.
    #[must_use]
    pub fn is_blob_referenced(&self, blob: &str) -> bool {
        self.records
            .values()
            .any(|record| record.attachments.iter().any(|a| a.blob == blob))
    }

    /// Whether a person is starred, given their cards: any one of them is.
    #[must_use]
    pub fn is_favorite(&self, cards: &[CardRef]) -> bool {
        cards
            .iter()
            .any(|card| self.records.get(card).is_some_and(|r| r.favorite))
    }

    /// Stars or unstars a person: every one of the given cards.
    ///
    /// Every card, so the star outlives unlinking and the deletion of any one
    /// of them. A card already in the wanted state is not rewritten, which
    /// also keeps unstarring from creating records for cards that had none.
    pub fn set_favorite(&mut self, cards: &[CardRef], favorite: bool) -> Result<(), String> {
        for card in cards {
            if self.records.get(card).is_some_and(|r| r.favorite) != favorite {
                self.change(card, |record| record.favorite = favorite)?;
            }
        }
        Ok(())
    }

    /// Puts a whole record back — the undo side of [`Self::forget`].
    ///
    /// An empty record writes nothing, so undoing the delete of a contact who
    /// had no notes does not create a file for them.
    pub fn restore(&mut self, card: &CardRef, record: Record) -> Result<(), String> {
        if record.is_empty() {
            return Ok(());
        }
        self.change(card, |slot| *slot = record)
    }

    /// Drops everything recorded against a card — for when the card itself is
    /// deleted, so notes about a contact who no longer exists do not outlive
    /// them silently.
    pub fn forget(&mut self, card: &CardRef) -> Result<(), String> {
        self.records.remove(card);
        let path = self.path_for(card);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(why) => Err(why.to_string()),
        }
    }

    /// Applies `edit` to a card's record and writes it. When the write fails
    /// the record is put back as it was, so memory never claims what the
    /// disk refused — a note shown as saved would be gone at the next launch.
    fn change(&mut self, card: &CardRef, edit: impl FnOnce(&mut Record)) -> Result<(), String> {
        let before = self.records.get(card).cloned();
        edit(self.records.entry(card.clone()).or_default());
        self.write(card).inspect_err(|_| match before {
            Some(record) => {
                self.records.insert(card.clone(), record);
            }
            None => {
                self.records.remove(card);
            }
        })
    }

    fn write(&self, card: &CardRef) -> Result<(), String> {
        let path = self.path_for(card);
        match self.records.get(card) {
            // An emptied record is a deleted file, not an empty one: the
            // directory should hold what was recorded and nothing else.
            None => match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(why) => Err(why.to_string()),
            },
            Some(record) if record.is_empty() => match std::fs::remove_file(&path) {
                Ok(()) => Ok(()),
                Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(why) => Err(why.to_string()),
            },
            // Crash-safe: a torn record is skipped on reload, and with it the
            // notes and the attachment references it held.
            Some(record) => {
                let text = serde_json::to_string_pretty(record).map_err(|why| why.to_string())?;
                cosmic_pim_core::atomic::write(&path, &text, None)
                    .map(|_| ())
                    .map_err(|why| format!("{}: {why}", path.display()))
            }
        }
    }

    fn path_for(&self, card: &CardRef) -> PathBuf {
        self.dir.join(format!("{}.json", encode_key(card)))
    }
}

/// A card's coordinates as one file name.
///
/// A book id or a UID can carry `/`, `@`, and worse — a UID is arbitrary text
/// — so both halves are percent-ish escaped and joined with a separator that
/// cannot appear in the escaped form. Without this a UID containing a slash
/// would write outside the directory.
fn encode_key(card: &CardRef) -> String {
    format!("{}~{}", escape(&card.book), escape(&card.uid))
}

/// Escapes one half of a key, injectively.
///
/// Every byte of a character that is not safe in a file name is written as
/// `_XX`, **the character's UTF-8 bytes** rather than its scalar value
/// truncated to one. Truncating is not injective: `α` (U+03B1) and `±`
/// (U+00B1) agree in their low byte, so they escaped alike and two people's
/// notes landed in one file — one person's history showing up on another's
/// card, which is the same failure as writing one record over another and
/// looks just as much like nothing being wrong.
///
/// `_` is not in the safe set, so it escapes itself and cannot be confused
/// with the marker it would otherwise be.
fn escape(value: &str) -> String {
    use std::fmt::Write as _;

    let mut out = String::with_capacity(value.len());
    let mut buffer = [0u8; 4];
    for c in value.chars() {
        if c.is_ascii_alphanumeric() || c == '-' || c == '.' {
            out.push(c);
        } else {
            for byte in c.encode_utf8(&mut buffer).as_bytes() {
                let _ = write!(out, "_{byte:02x}");
            }
        }
    }
    out
}

fn decode_key(stem: &str) -> Option<CardRef> {
    let (book, uid) = stem.split_once('~')?;
    Some(CardRef {
        book: unescape(book)?,
        uid: unescape(uid)?,
    })
}

/// The inverse of [`escape`].
///
/// Escaped bytes are gathered and decoded as UTF-8 together, not one at a
/// time: a multi-byte character is several `_XX` in a row, and turning each
/// byte into its own `char` would decode `α` as two Latin-1 characters and
/// silently file the record under an id nobody will look for again.
fn unescape(value: &str) -> Option<String> {
    let mut out = String::with_capacity(value.len());
    let mut bytes: Vec<u8> = Vec::new();
    let mut chars = value.chars();

    while let Some(c) = chars.next() {
        if c == '_' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() != 2 {
                return None;
            }
            bytes.push(u8::from_str_radix(&hex, 16).ok()?);
            continue;
        }
        if !bytes.is_empty() {
            out.push_str(&String::from_utf8(std::mem::take(&mut bytes)).ok()?);
        }
        out.push(c);
    }
    if !bytes.is_empty() {
        out.push_str(&String::from_utf8(bytes).ok()?);
    }
    Some(out)
}

/// What the detail pane and the overdue list need about one person: the union
/// of their cards' records.
///
/// Notes and interactions union because they are history, and history does not
/// belong to one card more than another. The cadence is the **head** card's,
/// because it is an intention about a person and taking the shortest or the
/// longest of several would be a rule nobody could predict.
#[derive(Debug, Default)]
pub struct Summary {
    pub notes: Vec<Note>,
    pub last_contacted: Option<DateTime<Utc>>,
    pub cadence_days: Option<u32>,
    /// Every card's attachments, newest first — history, like the notes.
    pub attachments: Vec<crate::attachments::Attachment>,
}

impl Summary {
    /// When the next contact was meant to happen.
    #[must_use]
    pub fn due(&self) -> Option<DateTime<Utc>> {
        let days = self.cadence_days?;
        Some(match self.last_contacted {
            Some(last) => last + chrono::Duration::days(i64::from(days)),
            // A cadence set and nothing ever logged is due now, not never:
            // setting one is how you say you have been meaning to.
            None => DateTime::<Utc>::MIN_UTC,
        })
    }

    /// Whether this person is past their cadence as of `now`.
    #[must_use]
    pub fn is_overdue(&self, now: DateTime<Utc>) -> bool {
        self.due().is_some_and(|due| due <= now)
    }
}

/// Builds the summary for a person, given their cards in precedence order.
#[must_use]
pub fn summarise(store: &CrmStore, cards: &[CardRef]) -> Summary {
    let mut summary = Summary::default();
    for (index, card) in cards.iter().enumerate() {
        let Some(record) = store.record(card) else {
            continue;
        };
        summary.notes.extend(record.notes.iter().cloned());
        summary
            .attachments
            .extend(record.attachments.iter().cloned());
        summary.last_contacted = summary.last_contacted.max(record.last_contacted());
        if index == 0 {
            summary.cadence_days = record.cadence_days;
        }
    }
    // Newest first: the last thing you wrote about somebody is the thing you
    // want to see when you open them.
    summary.notes.sort_by_key(|note| std::cmp::Reverse(note.at));
    summary
        .attachments
        .sort_by_key(|attachment| std::cmp::Reverse(attachment.added));
    summary
}

/// Moves the starred people to the head of a list of rows and says how many
/// they are. The order within each half is kept, so the list reads the way it
/// was sorted, twice over — GNOME Contacts' "Favorites" section.
///
/// A row stands for a person, and a person is starred when any card in their
/// link record is — not only the cards the current filter left visible, or a
/// star would come and go with the sidebar.
pub fn favorites_first(rows: &mut [Contact], store: &CrmStore, links: &LinkStore) -> usize {
    // The ordinary address book has no stars; do not resolve a person per
    // row to find that out.
    if !store.records.values().any(|record| record.favorite) {
        return 0;
    }
    let mut favorites = 0;
    rows.sort_by_cached_key(|row| {
        let starred = store.is_favorite(&links.cards_of_person(&row.addressbook_id, &row.uid));
        favorites += usize::from(starred);
        !starred
    });
    favorites
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(book: &str, uid: &str) -> CardRef {
        CardRef {
            book: book.into(),
            uid: uid.into(),
        }
    }

    fn at(days_ago: i64) -> DateTime<Utc> {
        Utc::now() - chrono::Duration::days(days_ago)
    }

    /// A store whose record for `c` cannot be written: its path is a
    /// directory with something in it, which no rename replaces.
    fn unwritable(dir: &Path, c: &CardRef) -> CrmStore {
        let store = CrmStore::open(dir);
        let path = store.path_for(c);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::write(path.join("keep"), b"").unwrap();
        store
    }

    /// A note that could not be saved must not be shown as saved: it would
    /// sit in the pane until the next launch and then be gone.
    #[test]
    fn a_failed_write_leaves_nothing_behind_in_memory() {
        let dir = tempfile::tempdir().unwrap();
        let c = card("personal", "ada");
        let mut store = unwritable(dir.path(), &c);

        assert!(store.add_note(&c, "met at the museum", at(0)).is_err());
        assert!(store.log_interaction(&c, "call", at(0)).is_err());
        assert!(store.set_cadence(&c, Some(30)).is_err());
        assert!(
            store.record(&c).is_none_or(Record::is_empty),
            "memory kept what the disk refused: {:?}",
            store.record(&c)
        );
        assert!(!store.has_any_cadence());
    }

    #[test]
    fn a_note_survives_reopening_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&ada, "Met at the conference", Utc::now())
            .unwrap();

        let reopened = CrmStore::open(dir.path());
        let notes = &reopened.record(&ada).unwrap().notes;
        assert_eq!(notes.len(), 1);
        assert_eq!(notes[0].text, "Met at the conference");
    }

    #[test]
    fn an_empty_note_is_not_recorded() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.add_note(&ada, "   ", Utc::now()).unwrap();
        assert!(store.record(&ada).is_none_or(Record::is_empty));
    }

    /// Emptying a record removes its file rather than leaving `{}` behind.
    #[test]
    fn removing_the_last_note_removes_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.add_note(&ada, "One", Utc::now()).unwrap();
        let id = store.record(&ada).unwrap().notes[0].id.clone();
        store.remove_note(&ada, &id).unwrap();

        assert!(CrmStore::open(dir.path()).record(&ada).is_none());
        assert!(
            std::fs::read_dir(dir.path().join(".crm"))
                .map(|d| d.count() == 0)
                .unwrap_or(true),
            "an emptied record left a file behind"
        );
    }

    /// Two records must never share a file.
    ///
    /// The file name is a map from (book, uid) to a string, and a map that is
    /// not injective silently merges the things it collides — here, one
    /// person's notes appearing on another person's card. `α` (U+03B1) and
    /// `±` (U+00B1) are the cheap demonstration: they differ only above the
    /// low byte, which is exactly what a truncating escape throws away.
    #[test]
    fn ids_that_differ_only_above_the_low_byte_get_different_files() {
        let dir = tempfile::tempdir().unwrap();
        let alpha = card("personal", "\u{3b1}");
        let plusminus = card("personal", "\u{b1}");
        assert_ne!(alpha, plusminus, "the fixture stopped testing two ids");

        let mut store = CrmStore::open(dir.path());
        store.add_note(&alpha, "About alpha", Utc::now()).unwrap();
        store
            .add_note(&plusminus, "About plus-minus", Utc::now())
            .unwrap();

        let files = std::fs::read_dir(dir.path().join(".crm")).unwrap().count();
        assert_eq!(files, 2, "two contacts' notes were written to one file");

        let reopened = CrmStore::open(dir.path());
        assert_eq!(
            reopened.record(&alpha).unwrap().notes[0].text,
            "About alpha"
        );
        assert_eq!(
            reopened.record(&plusminus).unwrap().notes[0].text,
            "About plus-minus"
        );
    }

    /// A uid is arbitrary text, and plenty of it is not ASCII. A note written
    /// against one has to still be there after a restart.
    #[test]
    fn a_non_ascii_id_round_trips_through_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let giorgos = card(
            "\u{3c0}\u{3b5}\u{3c1}\u{3c3}",
            "\u{393}\u{3b9}\u{3ce}\u{3c1}\u{3b3}\u{3bf}\u{3c2}@x",
        );

        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&giorgos, "Owes me a lyre lesson", Utc::now())
            .unwrap();

        let reopened = CrmStore::open(dir.path());
        let record = reopened
            .record(&giorgos)
            .expect("the record came back under the id it was written with");
        assert_eq!(record.notes[0].text, "Owes me a lyre lesson");
    }

    /// A UID is arbitrary text and routinely carries `@` and `/`; without
    /// escaping, one containing a slash would write outside the directory.
    #[test]
    fn awkward_ids_round_trip_through_the_file_name() {
        let dir = tempfile::tempdir().unwrap();
        let awkward = card("work/shared", "../../etc/passwd@host");
        let mut store = CrmStore::open(dir.path());
        store.add_note(&awkward, "Kept inside", Utc::now()).unwrap();

        let files: Vec<String> = std::fs::read_dir(dir.path().join(".crm"))
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(files.len(), 1, "{files:?}");
        assert!(
            !files[0].contains('/'),
            "the id escaped its directory: {files:?}"
        );

        let reopened = CrmStore::open(dir.path());
        assert_eq!(
            reopened.record(&awkward).unwrap().notes[0].text,
            "Kept inside"
        );
    }

    /// Linking two cards must union their history, the same way the detail
    /// view unions their addresses.
    #[test]
    fn a_persons_notes_are_the_union_of_their_cards() {
        let dir = tempfile::tempdir().unwrap();
        let (work, home) = (card("work", "ada"), card("personal", "ada"));
        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&work, "Reviewed the proposal", at(2))
            .unwrap();
        store.add_note(&home, "Birthday drinks", at(10)).unwrap();

        let summary = summarise(&store, &[work, home]);
        assert_eq!(summary.notes.len(), 2);
        assert_eq!(
            summary.notes[0].text, "Reviewed the proposal",
            "notes should be newest first"
        );
    }

    #[test]
    fn last_contacted_is_the_most_recent_across_every_card() {
        let dir = tempfile::tempdir().unwrap();
        let (work, home) = (card("work", "ada"), card("personal", "ada"));
        let mut store = CrmStore::open(dir.path());
        store.log_interaction(&work, "called", at(30)).unwrap();
        store.log_interaction(&home, "coffee", at(3)).unwrap();

        let summary = summarise(&store, &[work.clone(), home]);
        let last = summary.last_contacted.expect("an interaction was logged");
        assert!(
            (Utc::now() - last).num_days() == 3,
            "took the older interaction"
        );
    }

    /// The cadence is an intention about a person; taking the shortest or the
    /// longest of several cards would be a rule nobody could predict.
    #[test]
    fn the_cadence_comes_from_the_head_card() {
        let dir = tempfile::tempdir().unwrap();
        let (work, home) = (card("work", "ada"), card("personal", "ada"));
        let mut store = CrmStore::open(dir.path());
        store.set_cadence(&work, Some(30)).unwrap();
        store.set_cadence(&home, Some(90)).unwrap();

        assert_eq!(
            summarise(&store, &[work.clone(), home.clone()]).cadence_days,
            Some(30)
        );
        assert_eq!(summarise(&store, &[home, work]).cadence_days, Some(90));
    }

    #[test]
    fn somebody_within_their_cadence_is_not_overdue() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.set_cadence(&ada, Some(30)).unwrap();
        store.log_interaction(&ada, "called", at(5)).unwrap();

        assert!(!summarise(&store, &[ada]).is_overdue(Utc::now()));
    }

    #[test]
    fn somebody_past_their_cadence_is_overdue() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.set_cadence(&ada, Some(30)).unwrap();
        store.log_interaction(&ada, "called", at(45)).unwrap();

        assert!(summarise(&store, &[ada]).is_overdue(Utc::now()));
    }

    /// Setting a cadence is how you say you have been meaning to get in
    /// touch, so it counts from nothing rather than from never.
    #[test]
    fn a_cadence_with_nothing_logged_is_overdue_now() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.set_cadence(&ada, Some(30)).unwrap();

        assert!(summarise(&store, &[ada]).is_overdue(Utc::now()));
    }

    /// The sidebar entry is about cadences, so a note alone must not summon
    /// a list that can only be empty.
    #[test]
    fn a_note_alone_does_not_make_a_keep_in_touch_list() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.add_note(&ada, "Something", Utc::now()).unwrap();

        assert!(!store.is_empty(), "the note was not recorded");
        assert!(
            !store.has_any_cadence(),
            "a note conjured a keep-in-touch list"
        );

        store.set_cadence(&ada, Some(30)).unwrap();
        assert!(store.has_any_cadence());
    }

    #[test]
    fn nobody_without_a_cadence_is_ever_overdue() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.log_interaction(&ada, "called", at(4000)).unwrap();

        let summary = summarise(&store, &[ada]);
        assert!(summary.due().is_none());
        assert!(!summary.is_overdue(Utc::now()));
    }

    #[test]
    fn clearing_a_cadence_stops_it_counting() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.set_cadence(&ada, Some(30)).unwrap();
        store.set_cadence(&ada, None).unwrap();

        assert!(!summarise(&store, &[ada]).is_overdue(Utc::now()));
    }

    /// Notes about a contact who no longer exists must not outlive them
    /// silently.
    #[test]
    fn forgetting_a_card_removes_everything_recorded_against_it() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&ada, "Something private", Utc::now())
            .unwrap();
        store.forget(&ada).unwrap();

        assert!(CrmStore::open(dir.path()).record(&ada).is_none());
    }

    /// Deleting a contact is undoable, so forgetting their notes has to be
    /// too — otherwise undo brings back the card and loses what you wrote.
    #[test]
    fn a_forgotten_record_can_be_put_back() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&ada, "Met at the conference", Utc::now())
            .unwrap();
        store.set_cadence(&ada, Some(30)).unwrap();

        let kept = store.record(&ada).cloned().expect("a record to keep");
        store.forget(&ada).unwrap();
        store.restore(&ada, kept).unwrap();

        let reopened = CrmStore::open(dir.path());
        let record = reopened.record(&ada).expect("the record came back");
        assert_eq!(record.notes[0].text, "Met at the conference");
        assert_eq!(record.cadence_days, Some(30));
    }

    /// Undoing the delete of somebody who had no notes must not invent a file
    /// for them.
    #[test]
    fn restoring_an_empty_record_writes_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.restore(&ada, Record::default()).unwrap();

        assert!(store.record(&ada).is_none());
        assert!(!dir.path().join(".crm").join("personal~ada.json").exists());
    }

    #[test]
    fn a_star_survives_reopening_the_store() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        assert!(!store.is_favorite(std::slice::from_ref(&ada)));

        store
            .set_favorite(std::slice::from_ref(&ada), true)
            .unwrap();
        assert!(CrmStore::open(dir.path()).is_favorite(std::slice::from_ref(&ada)));
    }

    /// Unstarring the only thing on a record removes the file, and
    /// unstarring somebody who was never starred does not create one.
    #[test]
    fn unstarring_leaves_no_record_behind() {
        let dir = tempfile::tempdir().unwrap();
        let (ada, bob) = (card("personal", "ada"), card("personal", "bob"));
        let mut store = CrmStore::open(dir.path());
        store
            .set_favorite(std::slice::from_ref(&ada), true)
            .unwrap();
        store
            .set_favorite(&[ada.clone(), bob.clone()], false)
            .unwrap();

        assert!(!store.is_favorite(&[ada, bob.clone()]));
        assert!(
            store.record(&bob).is_none(),
            "a record was made for nothing"
        );
        assert!(
            std::fs::read_dir(dir.path().join(".crm"))
                .map(|d| d.count() == 0)
                .unwrap_or(true),
            "an emptied record left a file behind"
        );
    }

    /// A star is not a note: it must not disturb what else is recorded, and
    /// a record holding only a star is still a record worth keeping.
    #[test]
    fn a_star_sits_beside_the_notes_without_touching_them() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store.add_note(&ada, "Met at the museum", at(1)).unwrap();
        store
            .set_favorite(std::slice::from_ref(&ada), true)
            .unwrap();
        store
            .set_favorite(std::slice::from_ref(&ada), false)
            .unwrap();

        let reopened = CrmStore::open(dir.path());
        assert_eq!(reopened.record(&ada).unwrap().notes.len(), 1);
        assert!(!reopened.is_favorite(&[ada]));
    }

    /// A record written before favorites existed has no such field and
    /// reads as not starred.
    #[test]
    fn a_record_from_before_favorites_reads_as_not_starred() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".crm")).unwrap();
        std::fs::write(
            dir.path().join(".crm").join("personal~ada.json"),
            r#"{"notes":[],"interactions":[],"cadence_days":30,"attachments":[]}"#,
        )
        .unwrap();

        let store = CrmStore::open(dir.path());
        assert!(store.read_every_record());
        assert!(!store.is_favorite(&[card("personal", "ada")]));
    }

    /// Starring stars every card of the person, so the star is still there
    /// on whichever card a delete or an unlink leaves.
    #[test]
    fn a_star_is_on_every_card_of_the_person() {
        let dir = tempfile::tempdir().unwrap();
        let (work, home) = (card("work", "ada"), card("personal", "ada"));
        let mut store = CrmStore::open(dir.path());
        store
            .set_favorite(&[work.clone(), home.clone()], true)
            .unwrap();

        store.forget(&work).unwrap();
        assert!(store.is_favorite(std::slice::from_ref(&home)));
        assert!(
            store.is_favorite(&[work, home]),
            "any card starred is enough"
        );
    }

    /// Deleting a contact is undoable, and the undo has to bring the star
    /// back with the card — it travels in the record, like the notes.
    #[test]
    fn a_star_comes_back_with_an_undone_delete() {
        let dir = tempfile::tempdir().unwrap();
        let ada = card("personal", "ada");
        let mut store = CrmStore::open(dir.path());
        store
            .set_favorite(std::slice::from_ref(&ada), true)
            .unwrap();

        let kept = store.record(&ada).cloned().expect("a record to keep");
        store.forget(&ada).unwrap();
        assert!(!CrmStore::open(dir.path()).is_favorite(std::slice::from_ref(&ada)));

        store.restore(&ada, kept).unwrap();
        assert!(CrmStore::open(dir.path()).is_favorite(&[ada]));
    }

    /// A star that could not be written must not show as set.
    #[test]
    fn a_star_that_could_not_be_written_is_not_shown() {
        let dir = tempfile::tempdir().unwrap();
        let c = card("personal", "ada");
        let mut store = unwritable(dir.path(), &c);

        assert!(store.set_favorite(std::slice::from_ref(&c), true).is_err());
        assert!(!store.is_favorite(&[c]));
    }

    fn row(book: &str, uid: &str) -> Contact {
        let mut contact = Contact::draft(book);
        contact.uid = uid.to_owned();
        contact
    }

    fn uids(rows: &[Contact]) -> Vec<&str> {
        rows.iter().map(|c| c.uid.as_str()).collect()
    }

    /// Starred people lead the list; each half keeps the order it came in.
    #[test]
    fn starred_rows_lead_the_list_in_their_own_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CrmStore::open(dir.path());
        let links = LinkStore::open(dir.path());
        let mut rows = vec![
            row("personal", "ada"),
            row("personal", "bob"),
            row("personal", "cy"),
            row("personal", "dee"),
        ];
        assert_eq!(favorites_first(&mut rows, &store, &links), 0);
        assert_eq!(uids(&rows), ["ada", "bob", "cy", "dee"]);

        store
            .set_favorite(&[card("personal", "dee")], true)
            .unwrap();
        store
            .set_favorite(&[card("personal", "bob")], true)
            .unwrap();
        assert_eq!(favorites_first(&mut rows, &store, &links), 2);
        assert_eq!(uids(&rows), ["bob", "dee", "ada", "cy"]);
    }

    /// The row is the person. When the starred card is one the filter has
    /// hidden, the person's visible card still leads the list.
    #[test]
    fn a_row_is_starred_when_any_card_of_its_person_is() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CrmStore::open(dir.path());
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("work", "ada-w"), card("personal", "ada-p")])
            .unwrap();
        store.set_favorite(&[card("work", "ada-w")], true).unwrap();

        // Filtered to the personal book: the work card is not among the rows.
        let mut rows = vec![row("personal", "aaron"), row("personal", "ada-p")];
        assert_eq!(favorites_first(&mut rows, &store, &links), 1);
        assert_eq!(uids(&rows), ["ada-p", "aaron"]);
    }

    /// The whole point of `.crm/`: it is Circle's own data and must stay
    /// invisible to the vdir layer, or it would be listed as an address book
    /// and offered to a server as a collection.
    #[test]
    fn the_crm_store_never_becomes_an_address_book() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = CrmStore::open(dir.path());
        store
            .add_note(&card("personal", "ada"), "Note", Utc::now())
            .unwrap();

        assert!(
            cosmic_pim_core::store::contacts::books(dir.path()).is_empty(),
            "the CRM store leaked into the address-book list"
        );
    }
}
