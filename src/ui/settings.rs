// SPDX-License-Identifier: GPL-3.0-only

//! The settings context drawer.
//!
//! A pure function over the values it shows, so the shell hands it data and
//! gets an element back — nothing here reaches into the application model.
//! Every row corresponds to a field of [`crate::config::Config`], which is
//! the rule that keeps that struct honest: a setting nothing consults is a
//! promise the app does not keep.

use cosmic::Element;
use cosmic::widget;
use cosmic_pim_core::model::CalendarMeta;

use crate::app::Message;
use crate::config::Config;
use crate::fl;

/// The background sync cadences the dropdown offers, in minutes. Position-
/// matched to [`interval_labels`]; `0` is "never".
pub const SYNC_INTERVALS: [u32; 4] = [0, 15, 30, 60];

/// The cadence dropdown's labels.
///
/// A `LazyLock` because `widget::dropdown` borrows its labels for the lifetime
/// of the element it returns, so they cannot be built inside the view — and
/// resolved lazily because `fl!` needs the loader `main` initialises first.
pub static SYNC_INTERVAL_LABELS: std::sync::LazyLock<Vec<String>> =
    std::sync::LazyLock::new(|| {
        vec![
            fl!("sync-off"),
            fl!("sync-minutes", minutes = 15),
            fl!("sync-minutes", minutes = 30),
            fl!("sync-minutes", minutes = 60),
        ]
    });

/// The drawer's contents.
///
/// `writable_names` is borrowed for the returned element's lifetime, which is
/// why the caller caches it rather than building it here.
pub fn view<'a>(
    config: &'a Config,
    books: &'a [CalendarMeta],
    writable_ids: &'a [String],
    writable_names: &'a [String],
    // Whether accounts can exist at all — a sync cadence is meaningless
    // without somewhere to sync to.
    has_accounts: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(3).spacing(spacing.space_m);

    let default_book = config
        .default_book
        .as_ref()
        .and_then(|id| writable_ids.iter().position(|book| book == id));

    let mut general = widget::settings::section().title(fl!("view")).add(
        widget::settings::item::builder(fl!("sort-by-given-name"))
            .description(fl!("sort-by-given-name-description"))
            .toggler(config.sort_by_given_name, Message::SortByGivenName),
    );
    if !writable_names.is_empty() {
        general = general.add(
            widget::settings::item::builder(fl!("default-book")).control(widget::dropdown(
                writable_names,
                default_book,
                Message::DefaultBook,
            )),
        );
    }
    general = general.add(
        widget::settings::item::builder(fl!("prefer-vcard4"))
            .description(fl!("prefer-vcard4-description"))
            .toggler(config.prefer_vcard4, Message::PreferVcard4),
    );
    column = column.push(general);

    if has_accounts {
        let selected = SYNC_INTERVALS
            .iter()
            .position(|minutes| *minutes == config.sync_interval_minutes)
            .unwrap_or(0);
        column = column.push(
            widget::settings::section().title(fl!("sync")).add(
                widget::settings::item::builder(fl!("sync-interval"))
                    .description(fl!("sync-interval-description"))
                    .control(widget::dropdown(
                        &*SYNC_INTERVAL_LABELS,
                        Some(selected),
                        Message::SyncInterval,
                    )),
            ),
        );
    }

    if !books.is_empty() {
        let mut section = widget::settings::section().title(fl!("address-books"));
        for book in books {
            let id = book.id.clone();
            section = section.add(
                widget::settings::item::builder(book.name.clone())
                    .description(if book.read_only {
                        fl!("read-only-book", name = book.name.clone())
                    } else {
                        fl!("show-book")
                    })
                    .toggler(!config.is_hidden(&book.id), move |_| {
                        Message::ToggleBook(id.clone())
                    }),
            );
        }
        column = column.push(section);
    }

    column.into()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dropdown indexes into `SYNC_INTERVALS` by position, so a label
    /// added without its interval would silently set the wrong cadence.
    #[test]
    fn every_cadence_has_a_label() {
        assert_eq!(SYNC_INTERVALS.len(), SYNC_INTERVAL_LABELS.len());
    }

    #[test]
    fn the_first_cadence_is_off() {
        assert_eq!(SYNC_INTERVALS[0], 0);
    }
}
