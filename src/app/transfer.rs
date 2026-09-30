// SPDX-License-Identifier: GPL-3.0-only

//! Bringing cards in from `.vcf` and CSV files.

use cosmic::app::Task;

use super::save::save_error;
use super::sync::{queue_push, queue_push_with_base};
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

        match store.import_vcf(&text, &book_id) {
            Ok(summary) if summary.total() == 0 => {
                self.toast(fl!("import-empty", path = file_label(path)))
            }
            Ok(summary) => {
                let queued = summary
                    .files
                    .iter()
                    .map(|file| queue_push(store, &book_id, file))
                    .find_map(Result::err);
                // An updated card may carry a new photo under an old key.
                self.photos.clear();
                self.reload();
                let done = self.toast(fl!(
                    "import-done",
                    added = summary.added.to_string(),
                    updated = summary.updated.to_string()
                ));
                match queued {
                    Some(why) => Task::batch([done, self.toast(why)]),
                    None => done,
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
            if let Err(why) = store.save_as(&contact, version) {
                self.csv = None;
                self.reload();
                return self.toast(save_error(&contact.label(), &why));
            }
            if let Some(saved) = store.contact(&book_id, &contact.uid) {
                // An updated row adopted the existing card's bytes above;
                // those are its base. An added row has no before.
                let base = (!contact.raw.trim().is_empty()).then_some(contact.raw.as_str());
                if let Err(why) = queue_push_with_base(store, &book_id, &saved.file_name, base) {
                    queue_error.get_or_insert(why);
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

/// A path's file name, for messages — the full path is noise in a toast.
pub(super) fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}
