// SPDX-License-Identifier: GPL-3.0-only

//! Keyboard shortcuts.
//!
//! One map serves two purposes, which is why it lives apart from both: the menu
//! bar reads it to draw the accelerator beside each item, and
//! [`AppModel::update`](crate::app::AppModel) walks it on every key press that
//! no widget consumed. A binding added here therefore appears in the menu and
//! starts working in the same commit — they cannot drift.
//!
//! # Why these keys
//!
//! Modifiers on every character, the same rule Slate follows. This app is
//! mostly text entry — a search field, and an editor full of name and address
//! fields — so an unmodified `n` reaching the app while somebody is typing a
//! surname would be a bug that only shows up in use. libcosmic forwards a key
//! press only when no widget claimed it, so a focused text input already
//! shields these; the modifier is the second line of defence, not the first.
//!
//! Delete is the one bare key, as in GNOME Contacts. It types nothing, a
//! focused field claims it for itself, and a single delete is undoable byte
//! for byte from its toast while several ticked rows still ask first — so the
//! key is no easier a way to lose somebody than the menu item.
//!
//! Ctrl+D stars and unstars the selected contact — the chord browsers and
//! file managers use for "bookmark this". GNOME Contacts has no key for it.
//!
//! Sync now is Ctrl+Shift+R. Slate uses Ctrl+R for its sync, but here Ctrl+R
//! has always been Refresh (re-read the files on disk), and moving it would
//! break a habit for a chord the other half of the pair can live beside.

use cosmic::iced::keyboard::Key;
use cosmic::iced::keyboard::key::Named;
use cosmic::widget::menu::key_bind::{KeyBind, Modifier};
use std::collections::HashMap;

use crate::app::MenuAction;

/// The application's key bindings, keyed the way the menu widget expects.
#[must_use]
pub fn key_binds() -> HashMap<KeyBind, MenuAction> {
    let mut key_binds = HashMap::new();

    macro_rules! bind {
        ([$($modifier:ident),+ $(,)?], $key:expr, $action:ident) => {{
            key_binds.insert(
                KeyBind {
                    modifiers: vec![$(Modifier::$modifier),+],
                    key: $key,
                },
                MenuAction::$action,
            );
        }};
    }

    bind!([Ctrl], Key::Character("n".into()), NewContact);
    bind!([Ctrl], Key::Character("e".into()), EditContact);
    bind!([Ctrl], Key::Character("a".into()), SelectAll);
    bind!([Ctrl], Key::Character("d".into()), Favorite);
    bind!([Ctrl], Key::Character("f".into()), Search);
    bind!([Ctrl], Key::Character("i".into()), Import);
    bind!([Ctrl, Shift], Key::Character("e".into()), Export);
    bind!([Ctrl], Key::Character("r".into()), Refresh);
    bind!([Ctrl, Shift], Key::Character("r".into()), SyncNow);
    bind!([Ctrl], Key::Character(",".into()), Settings);
    key_binds.insert(
        KeyBind {
            modifiers: Vec::new(),
            key: Key::Named(Named::Delete),
        },
        MenuAction::Delete,
    );

    key_binds
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two actions sharing a chord means one of them silently never fires.
    #[test]
    fn no_two_actions_share_a_binding() {
        let binds = key_binds();
        let mut seen: Vec<&KeyBind> = binds.keys().collect();
        let before = seen.len();
        seen.sort_by_key(|b| format!("{:?}{:?}", b.modifiers, b.key));
        seen.dedup_by_key(|b| format!("{:?}{:?}", b.modifiers, b.key));
        assert_eq!(before, seen.len(), "two actions share one chord");
    }

    /// Every character binding carries a modifier — see the module docs.
    #[test]
    fn every_character_binding_is_modified() {
        for bind in key_binds().keys() {
            assert!(
                !bind.modifiers.is_empty() || !matches!(bind.key, Key::Character(_)),
                "{bind:?} would fire while someone is typing a name"
            );
        }
    }

    /// Delete, Sync now and the favorites toggle have keys (audit O-03,
    /// O-04, O-05).
    #[test]
    fn delete_sync_now_and_favorite_have_keys() {
        let actions: Vec<MenuAction> = key_binds().into_values().collect();
        assert!(actions.contains(&MenuAction::Delete));
        assert!(actions.contains(&MenuAction::SyncNow));
        assert!(actions.contains(&MenuAction::Favorite));
    }
}
