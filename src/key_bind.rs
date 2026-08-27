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
//! Modifiers on everything, the same rule Slate follows. This app is mostly
//! text entry — a search field, and an editor full of name and address fields —
//! so an unmodified `n` reaching the app while somebody is typing a surname
//! would be a bug that only shows up in use. libcosmic forwards a key press
//! only when no widget claimed it, so a focused text input already shields
//! these; the modifier is the second line of defence, not the first.
//!
//! Delete is deliberately absent. A contact list is a list of people, and the
//! bare Delete key beside a list is how you lose one — the menu item and its
//! confirmation dialog are the only route.

use cosmic::iced::keyboard::Key;
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
    bind!([Ctrl], Key::Character("f".into()), Search);
    bind!([Ctrl], Key::Character("i".into()), Import);
    bind!([Ctrl, Shift], Key::Character("e".into()), Export);
    bind!([Ctrl], Key::Character("r".into()), Refresh);
    bind!([Ctrl], Key::Character(",".into()), Settings);

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

    /// Every binding carries a modifier — see the module docs.
    #[test]
    fn every_binding_is_modified() {
        for bind in key_binds().keys() {
            assert!(
                !bind.modifiers.is_empty(),
                "{bind:?} would fire while someone is typing a name"
            );
        }
    }
}
