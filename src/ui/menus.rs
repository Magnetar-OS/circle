// SPDX-License-Identifier: GPL-3.0-only

//! The header menu bar.
//!
//! Every action Circle can perform appears here, whether or not it also has a
//! shortcut — a menu is the only place anybody discovers a feature exists, and
//! `menu::items` prints each entry's accelerator from the same
//! [`KeyBind`](cosmic::widget::menu::KeyBind) table
//! [`AppModel::update`](crate::app::AppModel) matches against, so a binding
//! and its menu entry cannot drift apart.
//!
//! Actions needing a selected contact are drawn **disabled** rather than
//! hidden: a menu whose items come and go teaches nobody where anything is.

use cosmic::prelude::*;
use cosmic::widget::menu;
use std::collections::HashMap;

use crate::app::{MenuAction, Message};
use crate::fl;

/// The bar, as `header_start` returns it.
pub fn bar(
    key_binds: &HashMap<menu::KeyBind, MenuAction>,
    // Whether anything is selected and editable — what the contact-specific
    // entries need to be more than decoration.
    can_edit: bool,
) -> Vec<Element<'_, Message>> {
    // Enabled or disabled, same label and same action: the accelerator still
    // prints, and the item stays where the user remembers it.
    let contact_item = |label: String, action: MenuAction| {
        if can_edit {
            menu::Item::Button(label, None, action)
        } else {
            menu::Item::ButtonDisabled(label, None, action)
        }
    };

    let file = menu::Tree::with_children(
        menu::root(fl!("file")).apply(Element::from),
        menu::items(
            key_binds,
            vec![
                menu::Item::Button(fl!("new-contact"), None, MenuAction::NewContact),
                menu::Item::Button(fl!("new-group"), None, MenuAction::NewGroup),
                menu::Item::Divider,
                menu::Item::Button(fl!("import"), None, MenuAction::Import),
                menu::Item::Button(fl!("import-csv"), None, MenuAction::ImportCsv),
                menu::Item::Button(fl!("export"), None, MenuAction::Export),
                menu::Item::Divider,
                menu::Item::Button(fl!("refresh"), None, MenuAction::Refresh),
                menu::Item::Button(fl!("sync-now"), None, MenuAction::SyncNow),
            ],
        ),
    );

    let edit = menu::Tree::with_children(
        menu::root(fl!("edit")).apply(Element::from),
        menu::items(
            key_binds,
            vec![
                contact_item(fl!("edit-contact"), MenuAction::EditContact),
                contact_item(fl!("delete-contact"), MenuAction::Delete),
                contact_item(fl!("share-contact"), MenuAction::Share),
                menu::Item::Divider,
                menu::Item::Button(fl!("select-all"), None, MenuAction::SelectAll),
                menu::Item::Button(fl!("find-duplicates"), None, MenuAction::Duplicates),
                menu::Item::Button(fl!("search-contacts"), None, MenuAction::Search),
            ],
        ),
    );

    let view = menu::Tree::with_children(
        menu::root(fl!("view")).apply(Element::from),
        menu::items(
            key_binds,
            vec![
                menu::Item::Button(fl!("accounts"), None, MenuAction::Accounts),
                menu::Item::Button(fl!("settings"), None, MenuAction::Settings),
                menu::Item::Button(fl!("about"), None, MenuAction::About),
            ],
        ),
    );

    vec![menu::bar(vec![file, edit, view]).into()]
}
