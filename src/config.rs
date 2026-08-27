// SPDX-License-Identifier: GPL-3.0-only

//! Persisted settings, stored through `cosmic-config` so they live alongside
//! every other COSMIC app's configuration and are picked up live when changed.
//!
//! Deliberately small. Every field here is read by the interface; a setting
//! nothing consults is a promise the app does not keep, which is the state the
//! README described before this file existed.

use cosmic::cosmic_config::{self, CosmicConfigEntry, cosmic_config_derive::CosmicConfigEntry};

#[derive(Clone, Debug, Default, CosmicConfigEntry, Eq, PartialEq)]
#[version = 1]
pub struct Config {
    /// Address books the user has unticked in the sidebar.
    ///
    /// Hidden, not deleted: the directory stays on disk and keeps syncing, it
    /// just does not contribute rows. Mirrors Slate's `hidden_calendars`.
    pub hidden_books: Vec<String>,

    /// Sort on the given name rather than the family name.
    ///
    /// Off by default because an address book that reads like a phone book is
    /// what most people expect, but the opposite convention is common enough —
    /// and strongly regional — that guessing would be wrong half the time.
    pub sort_by_given_name: bool,

    /// The book new contacts land in. `None` means the first writable one.
    ///
    /// Held as an id rather than an index so that adding an account, which
    /// reorders nothing but inserts a directory, cannot silently retarget it.
    pub default_book: Option<String>,

    /// Serialise **new** cards as vCard 4.0 rather than 3.0.
    ///
    /// Off by default: Nextcloud and most CardDAV servers are 3.0-first, and a
    /// 4.0 card handed to a 3.0-only peer is the interop failure users hit.
    /// Existing cards always keep the version their own bytes declare — the
    /// patcher never converts, so this switch touches only creation.
    pub prefer_vcard4: bool,

    /// Minutes between background sync passes; `0` means never.
    ///
    /// Off by default: an app that opens a network connection on a timer
    /// without being asked is a surprise, and an address book changes rarely
    /// enough that "when I press Sync" is a sensible default cadence.
    pub sync_interval_minutes: u32,
}

impl Config {
    #[must_use]
    pub fn is_hidden(&self, book_id: &str) -> bool {
        self.hidden_books.iter().any(|b| b == book_id)
    }

    pub fn toggle_book(&mut self, book_id: &str) {
        if let Some(pos) = self.hidden_books.iter().position(|b| b == book_id) {
            self.hidden_books.remove(pos);
        } else {
            self.hidden_books.push(book_id.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn toggling_a_book_is_reversible() {
        let mut config = Config::default();
        assert!(!config.is_hidden("work"));

        config.toggle_book("work");
        assert!(config.is_hidden("work"));

        config.toggle_book("work");
        assert!(!config.is_hidden("work"));
        assert!(config.hidden_books.is_empty());
    }

    #[test]
    fn nothing_is_hidden_by_default() {
        let config = Config::default();
        assert!(config.hidden_books.is_empty());
        assert!(config.default_book.is_none());
        assert!(!config.sort_by_given_name);
    }
}
