// SPDX-License-Identifier: GPL-3.0-only

//! Deleting contacts, and putting them back byte for byte.

use cosmic::app::Task;
use cosmic::widget;
use cosmic_pim_core::store::contacts::ContactStore;

use super::sync::write_and_queue;
use super::{AppModel, ContactKey, DeletedCard, Message, UNDO_DEPTH};
use crate::fl;

impl AppModel {
    /// Deletes the given contacts immediately and offers one undo toast for
    /// the lot. The cards' bytes are kept until the toast dies, so undo is a
    /// byte-for-byte restore, not a reconstruction.
    pub(super) fn delete_with_undo(&mut self, keys: Vec<ContactKey>) -> Task<Message> {
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };
        let root = store.root().to_path_buf();

        let mut removed = Vec::new();
        let mut label = String::new();
        let mut first_error: Option<String> = None;
        let mut side_error: Option<String> = None;
        for key in &keys {
            let Some(contact) = store.contact(&key.book, &key.uid) else {
                continue;
            };
            // The resource on the server is the file: when the card was the
            // whole file the resource goes, and when the file still holds
            // other cards (an export placed in a synced book) the rewritten
            // file is uploaded instead — a DELETE would take everybody else
            // in it off the server too.
            match write_and_queue(&root, &key.book, &[&contact.file_name], || {
                store.delete(&key.book, &key.uid)
            }) {
                Ok(((), queued)) => {
                    // Reported beside the undo toast, not instead of it: the
                    // card is already gone locally and must stay undoable.
                    if let Err(why) = queued {
                        side_error.get_or_insert(why);
                    }
                }
                Err(why) => {
                    first_error.get_or_insert_with(|| {
                        fl!(
                            "error-delete",
                            name = contact.label(),
                            why = why.to_string()
                        )
                    });
                    continue;
                }
            }
            self.photos.remove(key);
            label = contact.label();
            let card = crate::links::CardRef {
                book: key.book.clone(),
                uid: key.uid.clone(),
            };
            let crm = self.crm.record(&card).cloned();
            if let Err(why) = self.crm.forget(&card) {
                side_error.get_or_insert(why);
            }
            removed.push(DeletedCard {
                book: key.book.clone(),
                uid: key.uid.clone(),
                file_name: contact.file_name,
                // This card's own text, not the file's. `Contact::raw` is the
                // whole document, and a `.vcf` may hold several people — an
                // undo that wrote the document back would restore this card
                // and silently revert any edit made to the others since.
                raw: card_segment(&contact.raw, &key.uid),
                crm,
            });
        }

        self.selected = None;
        self.selecting = false;
        self.checked.clear();
        self.rebuild_nav();
        self.reload();

        // Whatever failed is reported, but never instead of the undo toast:
        // the cards that were removed must stay recoverable even when one of
        // a batch could not be deleted.
        let error = first_error.or(side_error);
        if removed.is_empty() {
            return error.map_or_else(Task::none, |why| self.toast(why));
        }

        let message =
            if let [card] = removed.as_slice() {
                // A linked person's row deletes the card it stands on — the same
                // one-card rule as editing. Saying "Deleted Ada" while Ada stays
                // in the list on her other card would be a lie.
                match self.store.as_ref().filter(|store| {
                    has_linked_cards_left(&self.links, store, &card.book, &card.uid)
                }) {
                    Some(store) => fl!(
                        "deleted-one-card",
                        name = label,
                        book = store
                            .book(&card.book)
                            .map_or_else(|| card.book.clone(), |b| b.name.clone())
                    ),
                    None => fl!("deleted-one", name = label),
                }
            } else {
                fl!("deleted-many", count = removed.len())
            };
        self.undo_seq += 1;
        let token = self.undo_seq;
        self.undo.insert(token, removed);
        while self.undo.len() > UNDO_DEPTH {
            let oldest = *self.undo.keys().next().unwrap_or(&token);
            self.undo.remove(&oldest);
        }
        let undo = self
            .toasts
            .push(
                widget::Toast::new(message)
                    .action(fl!("undo"), move |_| Message::UndoDelete(token)),
            )
            .map(Into::into);
        match error {
            Some(why) => Task::batch([undo, self.toast(why)]),
            None => undo,
        }
    }

    /// Puts a deletion's cards back, byte for byte, and re-queues them for
    /// upload — the mirror image of [`Self::delete_with_undo`].
    pub(super) fn undo_delete(&mut self, token: u64) -> Task<Message> {
        let Some(cards) = self.undo.remove(&token) else {
            return Task::none();
        };
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };
        let root = store.root().to_path_buf();

        let mut first_error = None;
        for card in cards {
            let Some(meta) = store.book(&card.book).cloned() else {
                first_error.get_or_insert_with(|| fl!("error-load-contacts"));
                continue;
            };
            let restoring = write_and_queue(&root, &card.book, &[&card.file_name], || {
                // Merged into the file's current contents rather than written
                // over them, for the same reason the segment was stored: the
                // people who shared this file with the deleted card are still
                // in it, and may have been edited since.
                let current = std::fs::read_to_string(meta.path.join(&card.file_name)).ok();
                let restored = restore_into(current.as_deref(), &card.raw, &card.uid);
                cosmic_pim_core::store::contacts::write_contact_raw(
                    &meta,
                    &card.file_name,
                    &restored,
                )
            });
            match restoring {
                Ok(((), queued)) => {
                    if let Err(why) = queued {
                        first_error.get_or_insert(why);
                    }
                    if let Some(record) = card.crm {
                        let card = crate::links::CardRef {
                            book: card.book.clone(),
                            uid: card.uid.clone(),
                        };
                        if let Err(why) = self.crm.restore(&card, record) {
                            first_error.get_or_insert(why);
                        }
                    }
                }
                Err(why) => {
                    first_error.get_or_insert_with(|| why.to_string());
                }
            }
        }

        self.rebuild_nav();
        self.reload();
        if let Some(why) = first_error {
            return self.toast(why);
        }
        Task::none()
    }
}

/// Whether the card at `(book, uid)` belongs to a linked person who still
/// has another card in the address book — so deleting it did not delete them.
fn has_linked_cards_left(
    links: &crate::links::LinkStore,
    store: &ContactStore,
    book: &str,
    uid: &str,
) -> bool {
    links.person_of(book, uid).is_some_and(|person| {
        person
            .cards
            .iter()
            .filter(|c| !(c.book == book && c.uid == uid))
            .any(|c| store.contact(&c.book, &c.uid).is_some())
    })
}

/// One card's own text, sliced out of a document that may hold several.
///
/// Thin wrapper over the substrate's, which owns the slicing. The fallback is
/// this application's decision: a document with no cards, or one this uid does
/// not name, keeps the original. For an undo that is the safe answer —
/// restoring too much is recoverable, restoring nothing is not — and for an
/// export it emits a card rather than nothing.
pub(super) fn card_segment(raw: &str, uid: &str) -> String {
    cosmic_pim_core::vcard::card_segment(raw, uid).unwrap_or_else(|| raw.to_owned())
}

/// A deleted card put back into whatever its file now holds.
///
/// Appended rather than re-inserted at its old position: the position is not
/// recorded and the order of cards in a `.vcf` carries no meaning, whereas
/// the edits made to the other cards since the delete very much do.
fn restore_into(current: Option<&str>, segment: &str, uid: &str) -> String {
    let Some(current) = current.map(str::trim_end).filter(|text| !text.is_empty()) else {
        return segment.to_owned();
    };
    // Already back — a sync pulled it, or the undo ran twice. Adding it again
    // would make two of them.
    //
    // Checked by looking for the UID itself, not with `vcard_index_of`: that
    // answers `Some(0)` for a lone card whatever its UID says, which is right
    // for locating the card to patch and wrong for asking whether a
    // particular one is present.
    if names_card(current, uid) {
        return current.to_owned();
    }
    format!("{current}\r\n{segment}")
}

/// Whether `text` holds a card carrying exactly this UID.
fn names_card(text: &str, uid: &str) -> bool {
    cosmic_pim_core::patch::logical_lines(text)
        .iter()
        .any(|line| line.name() == "UID" && line.value().trim() == uid)
}

#[cfg(test)]
mod tests {
    use super::*;

    const TWO: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nUID:bob\r\nFN:Bob\r\nEND:VCARD\r\n";

    /// `Contact::raw` is the whole file; an undo entry wants one card.
    #[test]
    fn a_segment_is_one_card_out_of_a_file_of_several() {
        let segment = card_segment(TWO, "bob");
        assert!(segment.contains("UID:bob"), "{segment}");
        assert!(
            !segment.contains("UID:ada"),
            "the segment took the other card too: {segment}"
        );
        assert_eq!(segment.matches("BEGIN:VCARD").count(), 1);
    }

    #[test]
    fn a_lone_card_is_its_own_segment() {
        let one = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n";
        assert_eq!(card_segment(one, "ada"), one);
    }

    /// The bug this pair exists to prevent: restoring Bob must not revert the
    /// edit made to Ada while the undo toast was up.
    #[test]
    fn restoring_a_card_keeps_the_edits_made_to_the_others() {
        let segment = card_segment(TWO, "bob");
        let after_delete_and_edit =
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n";

        let restored = restore_into(Some(after_delete_and_edit), &segment, "bob");
        assert!(
            restored.contains("FN:Ada Lovelace"),
            "the undo reverted the other card's edit: {restored}"
        );
        assert!(
            restored.contains("UID:bob"),
            "the deleted card did not come back"
        );
        assert_eq!(restored.matches("BEGIN:VCARD").count(), 2);
    }

    #[test]
    fn restoring_into_a_file_that_is_gone_writes_the_card_alone() {
        let segment = card_segment(TWO, "bob");
        assert_eq!(restore_into(None, &segment, "bob"), segment);
        assert_eq!(restore_into(Some("   "), &segment, "bob"), segment);
    }

    /// A sync may have pulled the card back before the undo ran; adding it
    /// again would make two of them.
    #[test]
    fn restoring_a_card_that_is_already_back_does_not_duplicate_it() {
        let segment = card_segment(TWO, "bob");
        let restored = restore_into(Some(TWO), &segment, "bob");
        assert_eq!(restored.matches("UID:bob").count(), 1, "{restored}");
    }

    /// Deleting the card a linked person's row stands on leaves the person
    /// in the list on their other card; the undo toast has to say so.
    #[test]
    fn a_linked_person_with_another_card_is_not_reported_as_deleted() {
        use crate::links::{CardRef, LinkStore};
        use cosmic_pim_core::store::contacts::write_contact_raw;

        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let home = store
            .create_book("Home", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        let work = store
            .create_book("Work", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        write_contact_raw(
            &home,
            "ada.vcf",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a\r\nFN:Ada\r\nEND:VCARD\r\n",
        )
        .unwrap();
        write_contact_raw(
            &work,
            "ada.vcf",
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:b\r\nFN:Ada\r\nEND:VCARD\r\n",
        )
        .unwrap();
        store.refresh();
        let mut links = LinkStore::open(dir.path());
        let card = |book: &str, uid: &str| CardRef {
            book: book.to_owned(),
            uid: uid.to_owned(),
        };
        links
            .link(vec![card(&home.id, "a"), card(&work.id, "b")])
            .unwrap();

        store.delete(&home.id, "a").unwrap();
        assert!(has_linked_cards_left(&links, &store, &home.id, "a"));

        store.delete(&work.id, "b").unwrap();
        assert!(
            !has_linked_cards_left(&links, &store, &work.id, "b"),
            "the last card of a person is the person"
        );
    }
}
