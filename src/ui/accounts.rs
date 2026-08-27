// SPDX-License-Identifier: GPL-3.0-only

//! The Accounts context page: add a CardDAV account, see its state, sync now.
//!
//! Mirrors Slate's page deliberately — the account store underneath is the
//! same file, so an account added in either app appears in both, and the two
//! forms asking the same four questions the same way is what makes that
//! believable. An account is a URL, a username, and a password; discovery
//! works out the rest, so the user can paste whatever their provider's help
//! page told them to.

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;

use crate::app::{AccountForm, Message};
use crate::fl;

/// One row per account, plus the add form or the button that opens it.
pub fn view<'a>(
    accounts: &'a [cosmic_pim_accounts::Account],
    form: Option<&'a AccountForm>,
    syncing: bool,
    status: Option<&'a str>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(5).spacing(spacing.space_s);

    if accounts.is_empty() && form.is_none() {
        column = column.push(
            widget::text::body(fl!("no-accounts-description"))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    }

    if !accounts.is_empty() {
        let mut list = widget::settings::section();
        for account in accounts {
            list = list.add(
                widget::settings::item::builder(account.display_name.clone())
                    .description(format!("{} · {}", account.username, account.url))
                    .control(
                        widget::button::text(fl!("remove"))
                            .class(cosmic::theme::Button::Destructive)
                            .on_press(Message::AccountRemove(account.id.clone())),
                    ),
            );
        }
        column = column.push(list);
    }

    column = match form {
        Some(form) => column.push(add_form(form)),
        None => column.push(
            widget::button::text(fl!("add-account"))
                .class(cosmic::theme::Button::Suggested)
                .on_press(Message::AccountAddStart),
        ),
    };

    if !accounts.is_empty() {
        let label = if syncing {
            fl!("syncing")
        } else {
            fl!("sync-now")
        };
        let button = widget::button::text(label);
        // No `on_press` while a pass is in flight: a second concurrent pass
        // would race the first one on the same sidecar files.
        column = column.push(if syncing {
            button
        } else {
            button.on_press(Message::SyncNow)
        });
    }

    if let Some(status) = status {
        column = column.push(
            widget::text::caption(status.to_owned())
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    }

    column.into()
}

fn add_form(form: &AccountForm) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();

    let section = widget::settings::section()
        .add(
            widget::settings::item::builder(fl!("account-name")).control(
                widget::text_input(fl!("account-name"), &form.display_name)
                    .on_input(Message::AccountNameChanged)
                    .width(Length::Fixed(220.0)),
            ),
        )
        .add(
            widget::settings::item::builder(fl!("server-url")).control(
                widget::text_input("https://…", &form.url)
                    .on_input(Message::AccountUrlChanged)
                    .width(Length::Fixed(220.0)),
            ),
        )
        .add(
            widget::settings::item::builder(fl!("username")).control(
                widget::text_input(fl!("username"), &form.username)
                    .on_input(Message::AccountUsernameChanged)
                    .width(Length::Fixed(220.0)),
            ),
        )
        .add(
            widget::settings::item::builder(fl!("password"))
                .description(fl!("app-password-hint"))
                .control(
                    widget::secure_input(fl!("password"), &form.password, None, true)
                        .on_input(Message::AccountPasswordChanged)
                        .width(Length::Fixed(220.0)),
                ),
        );

    let mut column = widget::column::with_capacity(3)
        .spacing(spacing.space_s)
        .push(section);

    if let Some(error) = &form.error {
        column = column.push(
            widget::text::body(error.clone())
                .class(cosmic::theme::Text::Custom(|theme| {
                    cosmic::iced::widget::text::Style {
                        color: Some(theme.cosmic().destructive_color().into()),
                        ..Default::default()
                    }
                }))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    }

    // A URL and a username are the minimum that could possibly work; an empty
    // password is left submittable because some servers genuinely use none.
    let can_submit = !form.url.trim().is_empty() && !form.username.trim().is_empty();

    column
        .push(
            widget::row::with_capacity(2)
                .spacing(spacing.space_xs)
                .push(widget::button::text(fl!("cancel")).on_press(Message::AccountAddCancel))
                .push({
                    let add =
                        widget::button::text(fl!("add")).class(cosmic::theme::Button::Suggested);
                    if can_submit {
                        add.on_press(Message::AccountAddConfirm)
                    } else {
                        add
                    }
                }),
        )
        .into()
}
