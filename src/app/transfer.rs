// SPDX-License-Identifier: GPL-3.0-only

//! Bringing cards in from `.vcf` and CSV files.

use cosmic::app::Task;
use cosmic_pim_core::store::ImportSummary;
use cosmic_pim_core::store::contacts::ContactStore;
use cosmic_pim_core::vcard::parse_vcards;

use super::save::save_error;
use super::sync::{queue_created, write_and_queue};
use super::{AppModel, Message};
use crate::fl;
use crate::ui::csv;

impl AppModel {
    /// Imports the cards in a `.vcf` file into the default book, UID-keyed so
    /// re-importing updates rather than duplicates.
    pub(super) fn import(&mut self, path: &std::path::Path) -> Task<Message> {
        let Some(book_id) = self
            .store
            .as_ref()
            .and_then(|s| self.config.new_card_book(s.books()))
        else {
            return self.toast(fl!("error-no-writable-book"));
        };

        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(why) => return self.toast(format!("{}: {why}", file_label(path))),
        };
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        match import_and_queue(store, &book_id, &text) {
            Ok((summary, _)) if summary.total() == 0 => {
                self.toast(fl!("import-empty", path = file_label(path)))
            }
            Ok((summary, queued)) => {
                // An updated card may carry a new photo under an old key.
                self.photos.clear();
                self.reload();
                let done = self.toast(fl!(
                    "import-done",
                    added = summary.added.to_string(),
                    updated = summary.updated.to_string()
                ));
                match queued {
                    Err(why) => Task::batch([done, self.toast(why)]),
                    Ok(()) => done,
                }
            }
            Err(why) => self.toast(why.to_string()),
        }
    }

    /// Commits the CSV mapping: every row becomes a contact in the default
    /// book; a row whose mapped UID already exists updates that contact
    /// through the patcher instead of duplicating it.
    pub(super) fn import_csv(&mut self) -> Task<Message> {
        let Some(state) = self.csv.as_ref() else {
            return Task::none();
        };
        if !state.is_importable() {
            return Task::none();
        }
        let version = self.write_version();
        let Some(book_id) = self
            .store
            .as_ref()
            .and_then(|s| self.config.new_card_book(s.books()))
        else {
            return self.toast(fl!("error-no-writable-book"));
        };

        let (contacts, skipped) = state.contacts(&book_id);
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };
        let root = store.root().to_path_buf();

        let mut added = 0usize;
        let mut updated = 0usize;
        let mut queue_error: Option<String> = None;
        for mut contact in contacts {
            // A mapped UID that already exists means "update that contact":
            // the row is laid over it, so the save patches losslessly and
            // clears nothing the CSV does not carry.
            if let Some(existing) = store.contact(&book_id, &contact.uid) {
                contact = csv::update_of(existing, contact);
                updated += 1;
            } else {
                added += 1;
            }
            match write_and_queue(&root, &book_id, &[&contact.file_name], || {
                store.save_as(&contact, version)
            }) {
                Ok(((), queued)) => {
                    if let Err(why) = queued {
                        queue_error.get_or_insert(why);
                    }
                }
                Err(why) => {
                    self.csv = None;
                    self.reload();
                    return self.toast(save_error(&contact.label(), &why));
                }
            }
        }

        self.csv = None;
        self.rebuild_nav();
        self.reload();
        let done = self.toast(fl!(
            "csv-import-done",
            added = added.to_string(),
            updated = updated.to_string(),
            skipped = skipped.to_string()
        ));
        match queue_error {
            Some(why) => Task::batch([done, self.toast(why)]),
            None => done,
        }
    }
}

/// Imports a `.vcf` document into `book_id` and queues every file it wrote
/// for upload; the queue's outcome as [`write_and_queue`] gives it.
///
/// The files of the cards this import updates are known before it runs, so
/// their upload is queued with the write, under the book's sync lock, each
/// with its pre-import text as the base. The files it adds get names it
/// picks as it goes; those are queued once it returns.
fn import_and_queue(
    store: &mut ContactStore,
    book_id: &str,
    text: &str,
) -> Result<(ImportSummary, Result<(), String>), cosmic_pim_sync::Error> {
    let root = store.root().to_path_buf();
    let mut updating: Vec<String> = parse_vcards(text, book_id, "")
        .iter()
        .filter_map(|card| store.contact(book_id, &card.uid))
        .map(|known| known.file_name)
        .collect();
    updating.sort();
    updating.dedup();
    let names: Vec<&str> = updating.iter().map(String::as_str).collect();

    let (summary, queued) =
        write_and_queue(&root, book_id, &names, || store.import_vcf(text, book_id))?;
    let queued = summary
        .files
        .iter()
        .filter(|file| !updating.contains(file))
        .map(|file| queue_created(store, book_id, file))
        .fold(queued, Result::and);
    Ok((summary, queued))
}

/// A path's file name, for messages — the full path is noise in a toast.
pub(super) fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_pim_caldav::push::{PushOp, PushQueue as _};
    use cosmic_pim_caldav::{CalDavStore as _, RemoteEvent, VdirStore};

    const ADA: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n";

    /// An import into a synced book: the card it updates is uploaded with
    /// the text it replaced as the merge base, and the card it adds is
    /// uploaded too.
    #[test]
    fn an_import_into_a_synced_book_queues_what_it_updated_and_added() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let book = store
            .create_book("Synced", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        let mut vdir = VdirStore::open_carddav(book.clone()).unwrap();
        vdir.set_remote("/dav/ab/", false).unwrap();
        vdir.upsert(&RemoteEvent {
            href: "/dav/ab/ada.vcf".into(),
            etag: "\"v1\"".into(),
            ics: ADA.into(),
        })
        .unwrap();
        store.refresh();

        let export = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nUID:bob\r\nFN:Bob\r\nEND:VCARD\r\n";
        let (summary, queued) = import_and_queue(&mut store, &book.id, export).unwrap();
        queued.unwrap();
        assert_eq!((summary.added, summary.updated), (1, 1));

        let pending = VdirStore::open(book).unwrap().pending().unwrap();
        assert_eq!(pending.len(), 2, "{pending:?}");
        let ada = pending
            .iter()
            .find(|p| p.op.href() == "/dav/ab/ada.vcf")
            .expect("the updated card was not queued");
        assert!(matches!(ada.op, PushOp::Put { .. }));
        assert_eq!(ada.base.as_deref(), Some(ADA), "the update lost its base");
        assert!(
            pending
                .iter()
                .any(|p| p.op.href() != "/dav/ab/ada.vcf" && matches!(p.op, PushOp::Put { .. })),
            "the added card was not queued: {pending:?}"
        );
    }
}
