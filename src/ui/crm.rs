// SPDX-License-Identifier: GPL-3.0-only

//! The keep-in-touch and notes sections of the detail pane.
//!
//! Both are drawn only for a person who is selected, and both say plainly that
//! what they hold is local: a note here never reaches a server, and somebody
//! else syncing the same address book will not see it. That is the promise
//! [`crate::crm`] keeps by storing outside the books, and it is worth stating
//! where it is being made rather than only in a module comment.

use cosmic::Element;
use cosmic::widget;

use crate::app::Message;
use crate::crm::Summary;
use crate::fl;

/// The cadences the dropdown offers, in days. Position-matched to
/// [`CADENCE_LABELS`]; `0` is "no reminder".
pub const CADENCES: [u32; 7] = [0, 7, 14, 30, 90, 182, 365];

/// The dropdown's labels.
///
/// A `LazyLock` because `widget::dropdown` borrows its labels for the lifetime
/// of the element it returns, so they cannot be built inside the view — and
/// resolved lazily because `fl!` needs the loader `main` initialises first.
pub static CADENCE_LABELS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
    vec![
        fl!("cadence-none"),
        fl!("cadence-weekly"),
        fl!("cadence-fortnightly"),
        fl!("cadence-monthly"),
        fl!("cadence-quarterly"),
        fl!("cadence-twice-yearly"),
        fl!("cadence-yearly"),
    ]
});

/// "Keep in touch": when you last were, how often you meant to be, and a
/// button to say you just have been.
pub fn keep_in_touch<'a>(
    summary: &Summary,
    now: chrono::DateTime<chrono::Utc>,
) -> Element<'a, Message> {
    let selected = CADENCES
        .iter()
        .position(|days| Some(*days) == summary.cadence_days.or(Some(0)))
        .unwrap_or(0);

    let last = match summary.last_contacted {
        Some(at) => fl!("last-contacted", when = relative_days(now, at)),
        None => fl!("last-contacted-never"),
    };

    let overdue = summary.is_overdue(now);

    // Overdue is said on the row that carries the date, not as a badge
    // elsewhere: the date is the thing it is a judgement about. The button
    // turns suggested with it, so the row states the problem and offers the
    // one action that answers it.
    let mut item = widget::settings::item::builder(fl!("last-contact")).description(last);
    if overdue {
        item = item.description(fl!("overdue"));
    }
    let log = if overdue {
        widget::button::suggested(fl!("log-interaction"))
    } else {
        widget::button::standard(fl!("log-interaction"))
    };

    widget::settings::section()
        .title(fl!("keep-in-touch"))
        .add(item.control(log.on_press(Message::LogInteraction)))
        .add(
            widget::settings::item::builder(fl!("cadence")).control(widget::dropdown(
                &*CADENCE_LABELS,
                Some(selected),
                Message::SetCadence,
            )),
        )
        .into()
}

/// The notes: newest first, each with the day it was written and a way to
/// take it back out.
pub fn notes<'a>(
    summary: &Summary,
    draft: &'a str,
    now: chrono::DateTime<chrono::Utc>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    let mut section = widget::settings::section().title(fl!("notes"));
    for note in &summary.notes {
        let id = note.id.clone();
        section = section.add(
            widget::settings::item::builder(note.text.clone())
                .description(relative_days(now, note.at))
                .control(widget::tooltip(
                    widget::button::icon(crate::ui::icon("list-remove-symbolic"))
                        .on_press(Message::RemoveNote(id)),
                    widget::text::body(fl!("remove")),
                    widget::tooltip::Position::Top,
                )),
        );
    }

    let mut add = widget::button::standard(fl!("add"));
    if !draft.trim().is_empty() {
        add = add.on_press(Message::AddNote);
    }

    widget::column::with_capacity(3)
        .spacing(spacing.space_xs)
        .push(section)
        .push(
            widget::row::with_capacity(2)
                .spacing(spacing.space_xs)
                .push(
                    widget::text_input(fl!("note-placeholder"), draft)
                        .on_input(Message::NoteInput)
                        .on_submit(|_| Message::AddNote)
                        .width(cosmic::iced::Length::Fill),
                )
                .push(add),
        )
        .push(
            widget::text::caption(fl!("notes-are-local"))
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        )
        .into()
}

/// "3 days ago", "today", "in 2 days" — a date somebody can read without
/// arithmetic.
///
/// Whole days rather than hours: the question these answer is "is this
/// recent", and an address book is not a stopwatch.
fn relative_days(
    now: chrono::DateTime<chrono::Utc>,
    then: chrono::DateTime<chrono::Utc>,
) -> String {
    let days = (now.date_naive() - then.date_naive()).num_days();
    match days {
        0 => fl!("when-today"),
        1 => fl!("when-yesterday"),
        // Fluent selects the plural; the count is passed as a number so it
        // can, which `format!("{n} days")` would not allow.
        d if d > 1 => fl!("when-days-ago", days = d),
        -1 => fl!("when-tomorrow"),
        d => {
            let days: i64 = -d;
            fl!("when-in-days", days = days)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The dropdown indexes into `CADENCES` by position, so a label without
    /// its interval would silently set the wrong cadence.
    #[test]
    fn every_cadence_has_a_label() {
        assert_eq!(CADENCES.len(), CADENCE_LABELS.len());
    }

    #[test]
    fn the_first_cadence_is_no_reminder() {
        assert_eq!(CADENCES[0], 0);
    }

    #[test]
    fn cadences_are_ordered_so_the_dropdown_reads_sensibly() {
        assert!(CADENCES.windows(2).all(|w| w[0] < w[1]), "{CADENCES:?}");
    }
}
