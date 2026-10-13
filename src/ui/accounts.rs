// SPDX-License-Identifier: GPL-3.0-only

//! The Accounts context page: add a CardDAV account, see its state, sync now.
//!
//! Mirrors Slate's page deliberately — the account store underneath is the
//! same file, so an account added in either app appears in both, and the two
//! forms asking the same four questions the same way is what makes that
//! believable. An account is a URL, a username, and a password; discovery
//! works out the rest, so the user can paste whatever their provider's help
//! page told them to.
//!
//! Sync conflicts are answered here too, laid out as Slate lays out its own:
//! the page that says "N to resolve" is the one that resolves them.

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;

use crate::app::{AccountForm, Message};
use crate::conflicts::{ConflictRow, Resolution};
use crate::fl;
use cosmic_pim_accounts::{Account, DavService, Registry};
use cosmic_pim_core::merge::{Overlap, Side};

/// One row per account, plus the add form or the button that opens it, and
/// any sync conflicts awaiting an answer.
pub fn view<'a>(
    accounts: &'a [cosmic_pim_accounts::Account],
    registry: &cosmic_pim_accounts::Registry,
    form: Option<&'a AccountForm>,
    syncing: bool,
    status: Option<&'a str>,
    conflicts: &'a [ConflictRow],
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
                    .description(description(account, registry))
                    .control(
                        widget::button::text(fl!("remove"))
                            .class(cosmic::theme::Button::Destructive)
                            .on_press(Message::AccountRemove(account.id.clone())),
                    ),
            );
        }
        column = column.push(list);
    }

    // The Accounts window first: it takes an address and works out the rest,
    // and the account it adds brings mail and calendars along where the
    // provider has them. The server form stays for a CardDAV server nobody's
    // address would lead to.
    column = match form {
        Some(form) => column.push(add_form(form)),
        None => column.push(
            widget::row::with_capacity(2)
                .spacing(spacing.space_xs)
                .push(
                    widget::button::text(fl!("add-account"))
                        .class(cosmic::theme::Button::Suggested)
                        .on_press(Message::OpenAccountsWindow),
                )
                .push(widget::button::text(fl!("add-server")).on_press(Message::AccountAddStart)),
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

    if !conflicts.is_empty() {
        column = column.push(conflicts_section(conflicts));
    }

    column.into()
}

/// What an account row says under the account's name: its login, where its
/// contacts come from, and, for one GNOME Online Accounts keeps signed in,
/// that it is kept there.
///
/// An account made from a provider carries no URL of its own (cosmic-pim 3
/// reads both of the provider's addresses from its manifest), and neither
/// does one linked from Online Accounts, so the row cannot print
/// `account.url` and asks where the sync pass will look:
/// [`Account::service_url`], the engine's own rule, so the two cannot
/// disagree. `None` there is an account with no address books — a mail-only
/// account, or a provider with no CardDAV, such as Microsoft.
fn description(account: &Account, registry: &Registry) -> String {
    let mut parts = vec![account.username.clone()];
    parts.push(
        account
            .service_url(registry, DavService::Contacts)
            .unwrap_or_else(|| fl!("account-no-contacts")),
    );
    if account.online_account().is_some() {
        parts.push(fl!("account-via-online-accounts"));
    }
    parts.join(" · ")
}

/// Unresolved sync conflicts: who each side says the card is, and the ways
/// out. Nothing resolves itself with time — the alternative to asking is
/// guessing.
fn conflicts_section(conflicts: &[ConflictRow]) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(2 + conflicts.len())
        .spacing(spacing.space_s)
        .push(widget::text::heading(fl!("conflicts")))
        .push(
            widget::text::caption(fl!("conflicts-description"))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    for (index, row) in conflicts.iter().enumerate() {
        column = column.push(conflict_card(index, row));
    }
    column.into()
}

/// One conflict: the wholesale answers, plus per-property choices when the
/// sync pass kept the revision both sides started from.
fn conflict_card(index: usize, row: &ConflictRow) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();

    let words = crate::conflicts::wording(row);
    let mut section = widget::settings::section().add(
        widget::settings::item::builder(words.summary)
            .description(row.book_name.clone())
            .control(
                widget::row::with_capacity(2)
                    .spacing(spacing.space_xxs)
                    .push(
                        widget::button::text(words.keep_mine)
                            .on_press(Message::ConflictResolve(index, Resolution::KeepMine)),
                    )
                    .push(
                        widget::button::text(words.take_theirs)
                            .on_press(Message::ConflictResolve(index, Resolution::TakeTheirs)),
                    ),
            ),
    );

    let Some(disputes) = &row.disputes else {
        return section.into();
    };

    if disputes.units.is_empty() {
        // The two edits touch different properties; nothing needs choosing.
        section = section.add(
            widget::settings::item::builder(fl!("conflict-merges-cleanly")).control(
                widget::button::suggested(fl!("conflict-merge-both"))
                    .on_press(Message::ConflictResolve(index, Resolution::Merge)),
            ),
        );
        return section.into();
    }

    let mut units = widget::column::with_capacity(disputes.units.len() + 2)
        .spacing(spacing.space_xs)
        .push(
            widget::text::caption(fl!("conflict-choose-description"))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    for (unit_index, overlap) in disputes.units.iter().enumerate() {
        units = units.push(dispute_unit(
            index,
            unit_index,
            overlap,
            disputes.choices.get(&overlap.unit).copied(),
        ));
    }
    let apply = widget::button::suggested(fl!("conflict-apply-merge"));
    units = units.push(if disputes.decided() {
        apply.on_press(Message::ConflictResolve(index, Resolution::Merge))
    } else {
        apply
    });

    section.add(units).into()
}

/// One disputed property: its name, what it said before either edit, and the
/// two versions to pick between — the chosen one drawn as the suggested
/// button.
fn dispute_unit(
    row: usize,
    unit: usize,
    overlap: &Overlap,
    chosen: Option<Side>,
) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();

    let side_button = |label: String, lines: Option<&Vec<String>>, side: Side| {
        let text = lines.map_or_else(|| fl!("conflict-absent"), |l| l.join("\n"));
        let body = widget::column::with_capacity(2)
            .spacing(spacing.space_xxxs)
            .push(widget::text::caption_heading(label))
            .push(widget::text::caption(text).wrapping(cosmic::iced::core::text::Wrapping::Word));
        widget::button::custom(body)
            .class(if chosen == Some(side) {
                cosmic::theme::Button::Suggested
            } else {
                cosmic::theme::Button::Standard
            })
            .padding(spacing.space_xxs)
            .width(Length::Fill)
            .on_press(Message::ConflictChoose(row, unit, side))
    };

    let mut column = widget::column::with_capacity(3)
        .spacing(spacing.space_xxxs)
        .push(widget::text::caption_heading(overlap.unit.clone()));
    if let Some(base) = &overlap.base {
        column = column.push(
            widget::text::caption(fl!("conflict-was", lines = base.join(" ")))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        );
    }
    column
        .push(
            widget::row::with_capacity(2)
                .spacing(spacing.space_xxs)
                .push(side_button(
                    fl!("conflict-mine"),
                    overlap.local.as_ref(),
                    Side::Local,
                ))
                .push(side_button(
                    fl!("conflict-theirs-label"),
                    overlap.remote.as_ref(),
                    Side::Remote,
                )),
        )
        .into()
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
    let can_submit =
        !form.adding && !form.url.trim().is_empty() && !form.username.trim().is_empty();

    column
        .push(
            widget::row::with_capacity(2)
                .spacing(spacing.space_xs)
                .push(widget::button::text(fl!("cancel")).on_press(Message::AccountAddCancel))
                .push({
                    let label = if form.adding {
                        fl!("adding-server")
                    } else {
                        fl!("add")
                    };
                    let add = widget::button::text(label).class(cosmic::theme::Button::Suggested);
                    if can_submit {
                        add.on_press(Message::AccountAddConfirm)
                    } else {
                        add
                    }
                }),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Built-in manifests only: an empty overlay directory.
    fn registry() -> (tempfile::TempDir, Registry) {
        let dir = tempfile::tempdir().unwrap();
        let registry = Registry::load_from(dir.path());
        (dir, registry)
    }

    /// cosmic-pim 3 leaves a provider account's URL empty. The row used to
    /// print that URL, so it showed nothing; it shows the provider's CardDAV
    /// address, which is where the sync pass now finds the address books.
    #[test]
    fn a_provider_account_shows_its_providers_contacts_address() {
        let (_dir, registry) = registry();
        let fastmail = registry.get("fastmail").unwrap();
        let account = fastmail.account_for("ada@fastmail.com");
        assert_eq!(account.url, "");

        let contacts = fastmail.contacts_url("ada@fastmail.com").unwrap();
        assert_eq!(
            description(&account, &registry),
            format!("ada@fastmail.com · {contacts}")
        );
    }

    /// An account cosmic-pim 2 made from a manifest carries the calendar
    /// address as its own. Its address books are not there, and the row does
    /// not say they are.
    #[test]
    fn an_account_stored_by_cosmic_pim_2_shows_contacts_not_calendars() {
        let (_dir, registry) = registry();
        let fastmail = registry.get("fastmail").unwrap();
        let mut account = fastmail.account_for("ada@fastmail.com");
        account.url = fastmail.calendar_url("ada@fastmail.com").unwrap();

        let contacts = fastmail.contacts_url("ada@fastmail.com").unwrap();
        assert_eq!(
            description(&account, &registry),
            format!("ada@fastmail.com · {contacts}")
        );
    }

    /// A server typed into the form is the address, provider or not.
    #[test]
    fn a_typed_server_shows_its_own_address() {
        let (_dir, registry) = registry();
        let account = Account::new("Home", "https://dav.example.com/", "ada");
        assert_eq!(
            description(&account, &registry),
            "ada · https://dav.example.com/"
        );
    }

    /// A Google account linked from GNOME Online Accounts has no URL and no
    /// password here; its row names Google's CardDAV address and where its
    /// sign-in is kept.
    #[test]
    fn an_online_account_says_where_it_signs_in() {
        let (_dir, registry) = registry();
        let account =
            Account::for_online_account("Google", "ada@gmail.com", "google", "account_1").unwrap();
        let contacts = registry
            .get("google")
            .unwrap()
            .contacts_url("ada@gmail.com")
            .unwrap();
        assert_eq!(
            description(&account, &registry),
            format!(
                "ada@gmail.com · {contacts} · {}",
                fl!("account-via-online-accounts")
            )
        );
    }

    /// An account with nowhere to find contacts says so rather than showing
    /// an empty address: Microsoft has no CardDAV, and a Gmail app-password
    /// account is mail only.
    #[test]
    fn an_account_without_address_books_says_so() {
        let (_dir, registry) = registry();
        let microsoft =
            Account::for_online_account("Microsoft", "ada@outlook.com", "microsoft", "account_2")
                .unwrap();
        let gmail = registry
            .get("google")
            .unwrap()
            .app_password_account("ada@gmail.com")
            .unwrap();

        for account in [&microsoft, &gmail] {
            assert!(
                description(account, &registry).contains(&fl!("account-no-contacts")),
                "{}",
                description(account, &registry)
            );
        }
    }
}
