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

        for old in &absorbed_ids {
            if *old != id {
                let _ = std::fs::remove_file(self.path_for(old));
            }
        }
        self.write_person(&person)?;
        self.persons.retain(|p| !absorbed_ids.contains(&p.id));
        self.persons.push(person);
        self.persons.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(())
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
            let _ = std::fs::remove_file(self.path_for(&person.id));
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

    fn write_file(&self, path: &Path, text: &str) -> Result<(), String> {
        std::fs::create_dir_all(&self.dir).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
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
