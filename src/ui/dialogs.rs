// SPDX-License-Identifier: GPL-3.0-only

//! The modal dialogs, and which one is up.
//!
//! Circle asks before doing something only when the thing is hard to take
//! back and hard to picture. A single delete does **not** appear here: it
//! happens immediately with an undo toast, because forgiveness is one click
//! and permission is a dialog every single time. Deleting several at once
//! does, because a set of ticked rows across three books is genuinely easy to
//! misread.
//!
//! The rest are dialogs because they need an answer typed or read, not
//! because they are dangerous.

use cosmic::prelude::*;
use cosmic::widget;

use crate::app::{ContactKey, Message};
use crate::fl;
use crate::ui::person::Composed;

#[derive(Clone, Debug)]
pub enum Dialog {
    /// Deleting several contacts at once — the one delete that still asks
    /// first, because eight rows ticked over three books is easy to misread.
    /// A single delete asks forgiveness instead: it happens immediately, with
    /// an undo toast.
    ConfirmDeleteMany {
        keys: Vec<ContactKey>,
    },
    /// Deleting a group card — separate from a contact delete because the
    /// body must say what is and is not lost (members stay).
    ConfirmDeleteGroup {
        key: ContactKey,
        name: String,
    },
    NewGroup {
        name: String,
    },
    /// Adding every checked contact to a `CATEGORIES` group by name.
    AddToGroup {
        name: String,
    },
    /// The QR code for one person, for a phone camera to read.
    Share {
        key: ContactKey,
    },
    /// Writing a text for a paired phone to send.
    Sms {
        number: String,
        body: String,
        /// The daemon has been asked and has not answered yet.
        sending: bool,
    },
}

/// The dialog that is up, if any.
///
/// `person` is the composed contact for a [`Dialog::Share`] — resolved by the
/// caller, because the dialogs module has no store — and `phone` names the
/// device an SMS would go through.
pub fn view<'a>(
    dialog: &'a Dialog,
    person: Option<&Composed<'_>>,
    phone: Option<&str>,
    checked: usize,
) -> Option<Element<'a, Message>> {
    Some(match dialog {
        Dialog::ConfirmDeleteMany { keys } => widget::dialog()
            .title(fl!("confirm-delete-many-title", count = keys.len()))
            .body(fl!("confirm-delete-body"))
            .primary_action(
                widget::button::destructive(fl!("delete")).on_press(Message::DeleteConfirmed),
            )
            .secondary_action(
                widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
            )
            .into(),
        Dialog::ConfirmDeleteGroup { name, .. } => widget::dialog()
            .title(fl!("confirm-delete-title", name = name.clone()))
            .body(fl!("confirm-delete-group-body"))
            .primary_action(
                widget::button::destructive(fl!("delete")).on_press(Message::DeleteConfirmed),
            )
            .secondary_action(
                widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
            )
            .into(),
        Dialog::NewGroup { name } => {
            let mut create = widget::button::suggested(fl!("create"));
            if !name.trim().is_empty() {
                create = create.on_press(Message::NewGroupConfirmed);
            }
            widget::dialog()
                .title(fl!("new-group"))
                .control(
                    widget::text_input(fl!("group-name"), name)
                        .on_input(Message::NewGroupName)
                        .on_submit(|_| Message::NewGroupConfirmed),
                )
                .primary_action(create)
                .secondary_action(
                    widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
                )
                .into()
        }
        // `key` is what the caller composed from; the arm itself only needs
        // the result.
        Dialog::Share { .. } => {
            let person = person?;
            widget::dialog()
                .title(fl!("share-contact"))
                .body(person.label.clone())
                .control(crate::ui::share::view::<Message>(person))
                .primary_action(
                    widget::button::standard(fl!("close")).on_press(Message::DialogCancel),
                )
                .into()
        }
        Dialog::Sms {
            number,
            body,
            sending,
        } => {
            let phone = phone.unwrap_or_default().to_owned();
            let mut send = widget::button::suggested(fl!("send"));
            if !body.trim().is_empty() && !*sending {
                send = send.on_press(Message::SmsSend);
            }
            widget::dialog()
                .title(fl!("sms-to", number = number.clone()))
                .body(fl!("sms-via", device = phone))
                .control(
                    widget::text_input(fl!("sms-body"), body)
                        .on_input(Message::SmsBody)
                        .on_submit(|_| Message::SmsSend),
                )
                .primary_action(send)
                .secondary_action(
                    widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
                )
                .into()
        }
        Dialog::AddToGroup { name } => {
            let mut add = widget::button::suggested(fl!("add"));
            if !name.trim().is_empty() {
                add = add.on_press(Message::AddToGroupConfirmed);
            }
            widget::dialog()
                .title(fl!("add-to-group-title", count = checked))
                .body(fl!("add-to-group-body"))
                .control(
                    widget::text_input(fl!("group-name"), name)
                        .on_input(Message::AddToGroupName)
                        .on_submit(|_| Message::AddToGroupConfirmed),
                )
                .primary_action(add)
                .secondary_action(
                    widget::button::standard(fl!("cancel")).on_press(Message::DialogCancel),
                )
                .into()
        }
    })
}
