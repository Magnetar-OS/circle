// SPDX-License-Identifier: GPL-3.0-only

//! The duplicate review screen.
//!
//! One pair at a time, side by side, with the evidence that put them here
//! spelled out. Two answers: **Link** — the default, and the only one offered
//! prominently — or **Not the same**, which is remembered so the pair is
//! never proposed again.
//!
//! There is deliberately no merge button. Merging is the one address-book
//! operation that destroys data the files can no longer reconstruct, and 03 §6
//! puts it behind a separate, explicit, undoable action rather than beside the
//! answer people will click without reading.

use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use cosmic_pim_core::model::Contact;

use crate::dedupe::{Candidate, Reason};
use crate::fl;

/// One card's side of the comparison, resolved by the caller — the review
/// screen has no store of its own.
pub struct Side<'a> {
    pub contact: &'a Contact,
    pub book: &'a str,
}

/// The screen. `resolved` pairs each candidate with the two cards it names,
/// in the same order; a candidate whose cards have gone (deleted underneath
/// the screen) is passed as `None` and simply not drawn.
pub fn view<'a, M: Clone + 'static>(
    candidates: &[Candidate],
    resolved: &[Option<(Side<'a>, Side<'a>)>],
    on_link: impl Fn(usize) -> M,
    on_ignore: impl Fn(usize) -> M,
) -> Element<'a, M> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(candidates.len() + 2).spacing(spacing.space_m);

    column = column.push(
        widget::text::body(fl!("review-explains-linking"))
            .wrapping(cosmic::iced::core::text::Wrapping::Word)
            .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
    );

    for (index, candidate) in candidates.iter().enumerate() {
        let Some(Some((left, right))) = resolved.get(index) else {
            continue;
        };
        column = column.push(pair(index, candidate, left, right, &on_link, &on_ignore));
    }

    widget::scrollable(column.padding(spacing.space_s))
        .height(Length::Fill)
        .into()
}

fn pair<'a, M: Clone + 'static>(
    index: usize,
    candidate: &Candidate,
    left: &Side<'a>,
    right: &Side<'a>,
    on_link: &impl Fn(usize) -> M,
    on_ignore: &impl Fn(usize) -> M,
) -> Element<'a, M> {
    let spacing = cosmic::theme::spacing();

    let evidence = match &candidate.reason {
        Reason::Email(value) => fl!("match-email", value = value.clone()),
        Reason::Phone(value) => fl!("match-phone", value = value.clone()),
        Reason::Name => fl!("match-name"),
    };

    let mut header = widget::row::with_capacity(2)
        .align_y(Alignment::Center)
        .spacing(spacing.space_xs)
        .push(widget::text::body(evidence));
    // A name match is a hunch and says so; the evidence-backed ones do not
    // need a badge, because their reason already names the value that matched.
    if !candidate.reason.is_strong() {
        header = header.push(crate::ui::icon("dialog-question-symbolic").size(spacing.space_s));
    }

    let body = widget::row::with_capacity(2)
        .spacing(spacing.space_s)
        .push(card(left))
        .push(card(right));

    let actions = widget::row::with_capacity(2)
        .spacing(spacing.space_xs)
        .push(widget::button::standard(fl!("not-the-same")).on_press(on_ignore(index)))
        .push(widget::button::suggested(fl!("link")).on_press(on_link(index)));

    widget::container(
        widget::column::with_capacity(3)
            .spacing(spacing.space_s)
            .push(header)
            .push(body)
            .push(actions),
    )
    .padding(spacing.space_s)
    .class(cosmic::theme::Container::Card)
    .into()
}

/// One card's summary: enough to tell two people apart, not the whole card.
fn card<'a, M: 'static>(side: &Side<'_>) -> Element<'a, M> {
    let spacing = cosmic::theme::spacing();
    let contact = side.contact;

    let mut column = widget::column::with_capacity(5)
        .spacing(spacing.space_xxxs)
        .push(widget::text::heading(contact.label()))
        .push(
            widget::text::caption(side.book.to_owned())
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );

    for email in &contact.emails {
        column = column.push(widget::text::caption(email.value.clone()));
    }
    for phone in &contact.phones {
        column = column.push(widget::text::caption(phone.value.clone()));
    }
    if let Some(organisation) = &contact.organisation {
        column = column.push(
            widget::text::caption(organisation.clone())
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    column.width(Length::FillPortion(1)).into()
}
