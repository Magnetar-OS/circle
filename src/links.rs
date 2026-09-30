// SPDX-License-Identifier: GPL-3.0-only

//! The link store: which cards are the same person.
//!
//! An app-level *person* is a set of links to underlying cards (03 §5). The
//! cards stay intact and sync unchanged to their own servers; the links are
//! Circle's own metadata, so they live beside the books rather than in them —
//! one JSON file per linked person under `$contacts_root/.links/`, a
//! dot-directory the collection scanner skips (and sync therefore never
//! provisions or pushes). Files as truth, greppable, consistent with the vdir
//! around them.
//!
//! A record may name a card that no longer exists — deleted, or its book
//! unhooked. That is not corruption: readers skip refs they cannot resolve,
//! and a person left with fewer than two live cards simply stops composing.
//! Nothing here garbage-collects eagerly, because an undone delete brings the
//! card back and the link with it.

use std::path::{Path, PathBuf};

use cosmic_pim_core::model::Contact;

/// One underlying card, by its coordinates.
#[derive(
    Clone, Debug, Eq, Hash, Ord, PartialEq, PartialOrd, serde::Serialize, serde::Deserialize,
)]
pub struct CardRef {
    pub book: String,
    pub uid: String,
}

/// One linked person: two or more cards, in precedence order — the first
/// card's fields win where fields collide.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Person {
    pub id: String,
    pub cards: Vec<CardRef>,
}

/// A candidate pair the user said is *not* the same person. Kept forever so
/// the review screen never re-asks a question it has already had answered.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
struct Ignored {
    pairs: Vec<(CardRef, CardRef)>,
}

/// [`LinkStore::fold`]'s result: the rows to show, and each head's other
/// cards.
#[derive(Debug, Default)]
pub struct Folded {
    pub rows: Vec<Contact>,
    pub members: std::collections::HashMap<CardRef, Vec<Contact>>,
}

pub struct LinkStore {
    dir: PathBuf,
    persons: Vec<Person>,
    ignored: Ignored,
}

impl LinkStore {
    /// Opens the store under the given contacts root, creating nothing until
    /// the first write.
    #[must_use]
    pub fn open(contacts_root: &Path) -> Self {
        let dir = contacts_root.join(".links");
        let mut store = Self {
            dir,
            persons: Vec::new(),
            ignored: Ignored::default(),
        };
        store.reload();
        store
    }

    /// Re-reads every record from disk.
    pub fn reload(&mut self) {
        self.persons.clear();
        self.ignored = Ignored::default();

        let Ok(entries) = std::fs::read_dir(&self.dir) else {
            return; // No directory yet: nothing linked, which is fine.
        };
        for entry in entries.filter_map(Result::ok) {
            let path = entry.path();
            if path.file_name().is_some_and(|n| n == "ignored.json") {
                match std::fs::read_to_string(&path).map(|t| serde_json::from_str(&t)) {
                    Ok(Ok(ignored)) => self.ignored = ignored,
                    Ok(Err(why)) => tracing::warn!(%why, "unreadable ignored-pairs record"),
                    Err(why) => tracing::warn!(%why, "unreadable ignored-pairs record"),
                }
                continue;
            }
            if path.extension().is_none_or(|e| e != "json") {
                continue;
            }
            match std::fs::read_to_string(&path).map(|t| serde_json::from_str::<Person>(&t)) {
                Ok(Ok(person)) if person.cards.len() >= 2 => self.persons.push(person),
                // A one-card person is a no-op record; skip it rather than
                // let it fold rows to themselves.
                Ok(Ok(_)) => {}
                Ok(Err(why)) => tracing::warn!(?path, %why, "skipping an unreadable link record"),
                Err(why) => tracing::warn!(?path, %why, "skipping an unreadable link record"),
            }
        }
        // Directory order is arbitrary; a stable order keeps composition
        // deterministic across runs.
        self.persons.sort_by(|a, b| a.id.cmp(&b.id));
    }

    #[must_use]
    pub fn persons(&self) -> &[Person] {
        &self.persons
    }

    /// The person a card belongs to, if any.
    #[must_use]
    pub fn person_of(&self, book: &str, uid: &str) -> Option<&Person> {
        self.persons
            .iter()
            .find(|p| p.cards.iter().any(|c| c.book == book && c.uid == uid))
    }

    /// Every card of the person this card belongs to, in precedence order —
    /// or the card alone, when it is nobody's but its own.
    #[must_use]
    pub fn cards_of_person(&self, book: &str, uid: &str) -> Vec<CardRef> {
        self.person_of(book, uid).map_or_else(
            || {
                vec![CardRef {
                    book: book.to_owned(),
                    uid: uid.to_owned(),
                }]
            },
            |person| person.cards.clone(),
        )
    }

    /// One row per person: every linked card present in `contacts` folds
    /// under its person's head, which is the first card in the person's
    /// record order that is present. Unlinked cards pass through. Rows keep
    /// their input order; the folded cards are returned keyed by their head.
    #[must_use]
    pub fn fold(&self, contacts: Vec<Contact>) -> Folded {
        let mut folded = Folded::default();
        if self.persons.is_empty() {
            folded.rows = contacts;
            return folded;
        }
        let present = |card: &CardRef| {
            contacts
                .iter()
                .any(|c| c.addressbook_id == card.book && c.uid == card.uid)
        };
        let heads: std::collections::HashMap<&str, CardRef> = self
            .persons
            .iter()
            .filter_map(|person| {
                let head = person.cards.iter().find(|card| present(card))?;
                Some((person.id.as_str(), head.clone()))
            })
            .collect();

        for contact in contacts {
            let card = CardRef {
                book: contact.addressbook_id.clone(),
                uid: contact.uid.clone(),
            };
            let head = self
                .person_of(&card.book, &card.uid)
                .and_then(|person| heads.get(person.id.as_str()));
            match head {
                Some(head) if *head != card => {
                    folded
                        .members
                        .entry(head.clone())
                        .or_default()
                        .push(contact);
                }
                _ => folded.rows.push(contact),
            }
        }
        folded
    }

    /// Links the given cards into one person, merging any persons they
    /// already belong to. Order matters: the first card becomes the
    /// precedence head unless it already sits inside an absorbed person that
    /// put another card first.
    pub fn link(&mut self, cards: Vec<CardRef>) -> Result<(), String> {
        // Gather every card involved: the named ones plus the full membership
        // of any person they already belong to, keeping first-seen order.
        let mut merged: Vec<CardRef> = Vec::new();
        let mut absorbed_ids: Vec<String> = Vec::new();
        for card in cards {
            if let Some(person) = self.person_of(&card.book, &card.uid) {
                if !absorbed_ids.contains(&person.id) {
                    absorbed_ids.push(person.id.clone());
                    for member in person.cards.clone() {
                        if !merged.contains(&member) {
                            merged.push(member);
                        }
                    }
                }
            } else if !merged.contains(&card) {
                merged.push(card);
            }
        }
        if merged.len() < 2 {
            return Ok(()); // Linking one card to itself is a no-op.
        }

        // Reuse the first absorbed record's identity so re-linking does not
        // churn file names; otherwise mint one.
        let id = absorbed_ids
            .first()
            .cloned()
            .unwrap_or_else(|| uuid::Uuid::new_v4().simple().to_string());
        let person = Person {
            id: id.clone(),
            cards: merged,
        };

        // The merged record is made durable before any absorbed one goes:
        // a failure in between leaves a card in two records, which the next
        // merge absorbs again, rather than in none.
        self.write_person(&person)?;
        let removed = absorbed_ids
            .iter()
            .filter(|old| **old != id)
            .try_for_each(|old| self.remove_record(old));
        self.persons.retain(|p| !absorbed_ids.contains(&p.id));
        self.persons.push(person);
        self.persons.sort_by(|a, b| a.id.cmp(&b.id));
        removed
    }

    /// Takes one card out of its person. A person left with one card is
    /// dissolved — a link to nothing is noise.
    pub fn unlink(&mut self, book: &str, uid: &str) -> Result<(), String> {
        let Some(index) = self
            .persons
            .iter()
            .position(|p| p.cards.iter().any(|c| c.book == book && c.uid == uid))
        else {
            return Ok(());
        };

        let mut person = self.persons[index].clone();
        person.cards.retain(|c| !(c.book == book && c.uid == uid));

        if person.cards.len() < 2 {
            self.remove_record(&person.id)?;
            self.persons.remove(index);
        } else {
            self.write_person(&person)?;
            self.persons[index] = person;
        }
        Ok(())
    }

    /// Records that two cards are not the same person.
    pub fn ignore(&mut self, a: CardRef, b: CardRef) -> Result<(), String> {
        if self.is_ignored(&a, &b) {
            return Ok(());
        }
        self.ignored.pairs.push((a, b));
        let text = serde_json::to_string_pretty(&self.ignored).map_err(|e| e.to_string())?;
        self.write_file(&self.dir.join("ignored.json"), &text)
    }

    #[must_use]
    pub fn is_ignored(&self, a: &CardRef, b: &CardRef) -> bool {
        self.ignored
            .pairs
            .iter()
            .any(|(x, y)| (x == a && y == b) || (x == b && y == a))
    }

    fn path_for(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.json"))
    }

    fn write_person(&self, person: &Person) -> Result<(), String> {
        let text = serde_json::to_string_pretty(person).map_err(|e| e.to_string())?;
        self.write_file(&self.path_for(&person.id), &text)
    }

    /// Crash-safe: a torn record would be skipped on reload, silently
    /// unlinking its cards.
    fn write_file(&self, path: &Path, text: &str) -> Result<(), String> {
        cosmic_pim_core::atomic::write(path, text, None)
            .map(|_| ())
            .map_err(|e| format!("{}: {e}", path.display()))
    }

    /// Removes a person's record; one that is already gone is not an error.
    fn remove_record(&self, id: &str) -> Result<(), String> {
        let path = self.path_for(id);
        match std::fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(why) => Err(format!("{}: {why}", path.display())),
        }
    }
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

    #[test]
    fn linking_two_cards_makes_one_person_that_survives_a_reload() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();

        let reopened = LinkStore::open(dir.path());
        assert_eq!(reopened.persons().len(), 1);
        assert!(reopened.person_of("work", "b").is_some());
        assert!(reopened.person_of("personal", "a").is_some());
    }

    #[test]
    fn linking_into_an_existing_person_merges_rather_than_duplicates() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        links
            .link(vec![card("personal", "a"), card("shared", "c")])
            .unwrap();

        assert_eq!(links.persons().len(), 1);
        assert_eq!(links.persons()[0].cards.len(), 3);
    }

    #[test]
    fn linking_two_persons_absorbs_both() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        links
            .link(vec![card("shared", "c"), card("other", "d")])
            .unwrap();
        links
            .link(vec![card("personal", "a"), card("shared", "c")])
            .unwrap();

        let reopened = LinkStore::open(dir.path());
        assert_eq!(reopened.persons().len(), 1);
        assert_eq!(reopened.persons()[0].cards.len(), 4);
    }

    #[test]
    fn unlinking_below_two_cards_dissolves_the_person() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        links.unlink("work", "b").unwrap();

        assert!(links.persons().is_empty());
        let reopened = LinkStore::open(dir.path());
        assert!(reopened.persons().is_empty());
    }

    /// Merging two persons rewrites the first record and removes the second.
    /// When the rewrite fails, the second must still be on disk: removing it
    /// first left its cards unlinked with nothing to show for it.
    #[test]
    fn a_failed_merge_leaves_the_absorbed_record_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        links
            .link(vec![card("shared", "c"), card("other", "d")])
            .unwrap();
        let first = links.person_of("personal", "a").unwrap().id.clone();
        let second = links.person_of("shared", "c").unwrap().id.clone();

        // The merged record cannot be written: its path is now a directory
        // with something in it, which neither a write nor a rename replaces.
        let merged_path = links.path_for(&first);
        std::fs::remove_file(&merged_path).unwrap();
        std::fs::create_dir(&merged_path).unwrap();
        std::fs::write(merged_path.join("keep"), b"").unwrap();

        assert!(
            links
                .link(vec![card("personal", "a"), card("shared", "c")])
                .is_err()
        );
        assert!(
            links.path_for(&second).exists(),
            "the absorbed record was removed before the merged one was written"
        );
        assert_eq!(links.persons().len(), 2, "memory moved ahead of the disk");
    }

    /// A record that could not be removed would link its cards again on the
    /// next reload, so the failure has to reach the caller.
    #[test]
    fn a_record_that_cannot_be_removed_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        links
            .link(vec![card("shared", "c"), card("other", "d")])
            .unwrap();
        let second = links.person_of("shared", "c").unwrap().id.clone();

        let stuck = links.path_for(&second);
        std::fs::remove_file(&stuck).unwrap();
        std::fs::create_dir(&stuck).unwrap();
        std::fs::write(stuck.join("keep"), b"").unwrap();

        assert!(
            links
                .link(vec![card("personal", "a"), card("shared", "c")])
                .is_err(),
            "a failed removal of an absorbed record was swallowed"
        );
    }

    /// The same rule for unlinking: dissolving a person whose record cannot
    /// be removed is not a success.
    #[test]
    fn dissolving_a_person_whose_record_cannot_be_removed_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();
        let id = links.person_of("personal", "a").unwrap().id.clone();

        let stuck = links.path_for(&id);
        std::fs::remove_file(&stuck).unwrap();
        std::fs::create_dir(&stuck).unwrap();
        std::fs::write(stuck.join("keep"), b"").unwrap();

        assert!(links.unlink("work", "b").is_err());
        assert_eq!(links.persons().len(), 1, "memory moved ahead of the disk");
    }

    fn contact(book: &str, uid: &str) -> Contact {
        let mut c = Contact::draft(book);
        c.uid = uid.to_owned();
        c
    }

    /// A linked person is one row, headed by the first card in record order
    /// that is present — so a filter that hides the head promotes the next.
    #[test]
    fn folding_leaves_one_row_per_person() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();

        let all = links.fold(vec![
            contact("work", "b"),
            contact("personal", "a"),
            contact("personal", "c"),
        ]);
        let rows: Vec<&str> = all.rows.iter().map(|c| c.uid.as_str()).collect();
        assert_eq!(rows, ["a", "c"]);
        assert_eq!(all.members[&card("personal", "a")][0].uid, "b");

        let without_head = links.fold(vec![contact("work", "b")]);
        assert_eq!(without_head.rows.len(), 1);
        assert!(without_head.members.is_empty());
    }

    #[test]
    fn an_ignored_pair_is_remembered_in_either_order() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .ignore(card("personal", "a"), card("work", "b"))
            .unwrap();

        let reopened = LinkStore::open(dir.path());
        assert!(reopened.is_ignored(&card("work", "b"), &card("personal", "a")));
    }

    #[test]
    fn the_link_directory_is_hidden_from_the_collection_scanner() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();

        assert!(
            cosmic_pim_core::store::contacts::books(dir.path()).is_empty(),
            "the link store leaked into the address-book list"
        );
    }
}
