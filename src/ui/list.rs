// SPDX-License-Identifier: GPL-3.0-only

//! The contact list and the detail pane.

use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit};
use cosmic::iced::{Alignment, Length};
use cosmic::prelude::*;
use cosmic::widget;
use cosmic_pim_core::model::Contact;

use crate::app::{ContactKey, Message};
use crate::fl;

/// The scrollable list of contacts.
pub fn list<'a>(
    contacts: &'a [Contact],
    selected: Option<&'a ContactKey>,
    query: &str,
    photos: &'a std::collections::HashMap<ContactKey, widget::image::Handle>,
    selecting: bool,
    checked: &std::collections::HashSet<ContactKey>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    if contacts.is_empty() {
        let message = if query.trim().is_empty() {
            fl!("no-contacts")
        } else {
            fl!("no-search-results", query = query.trim().to_owned())
        };
        return widget::container(
            widget::text::body(message).class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        )
        .padding(spacing.space_m)
        .into();
    }

    let mut column = widget::column::with_capacity(contacts.len()).spacing(spacing.space_xxxs);
    for contact in contacts {
        let key = ContactKey::of(contact);
        let is_selected = selected.is_some_and(|key| key.matches(contact));
        let photo = photos.get(&key);
        let check = selecting.then_some(checked.contains(&key));
        column = column.push(row(contact, is_selected, photo, check));
    }

    widget::scrollable(column.padding(spacing.space_xxs))
        .height(Length::Fill)
        .into()
}

fn row<'a>(
    contact: &'a Contact,
    selected: bool,
    photo: Option<&widget::image::Handle>,
    // `None` outside selection mode; `Some(ticked)` inside it.
    check: Option<bool>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();

    // The secondary line is whatever identifies this person best after their
    // name: the preferred email, else a phone, else where they work. Showing a
    // blank line for a contact with none of those looks like a rendering bug.
    let secondary = Contact::preferred(&contact.emails)
        .map(|e| e.value.clone())
        .or_else(|| Contact::preferred(&contact.phones).map(|p| p.value.clone()))
        .or_else(|| contact.organisation.clone())
        .unwrap_or_default();

    // The name and the secondary line are other people's data and can be any
    // length; ellipsize rather than wrap, or one long organisation name makes
    // its row three lines tall and the list ragged.
    let one_line = Ellipsize::End(EllipsizeHeightLimit::Lines(1));
    let mut text = widget::column::with_capacity(2)
        .push(widget::text::body(contact.label()).ellipsize(one_line));
    if !secondary.is_empty() {
        text = text.push(
            widget::text::caption(secondary)
                .ellipsize(one_line)
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    let mut content = widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .spacing(spacing.space_xs);
    if let Some(ticked) = check {
        content = content.push(crate::ui::icon(if ticked {
            "checkbox-checked-symbolic"
        } else {
            "checkbox-symbolic"
        }));
    }
    let ticked = check == Some(true);

    widget::button::custom(
        content
            .push(crate::ui::avatar::avatar(
                photo,
                &contact.label(),
                f32::from(spacing.space_l),
            ))
            .push(text)
            .width(Length::Fill),
    )
    .class(if ticked || (selected && check.is_none()) {
        cosmic::theme::Button::Suggested
    } else {
        cosmic::theme::Button::Text
    })
    .padding([spacing.space_xxs, spacing.space_xs])
    .width(Length::Fill)
    .on_press(Message::Select(ContactKey::of(contact)))
    .into()
}

/// The detail pane for one person — one card, or several linked into one.
///
/// # Why every value here is selectable
///
/// The single most common thing anybody does with an address book is take a
/// value out of it — copy a phone number into a dialler, an address into a
/// form. Rendered as plain [`widget::text`] these values cannot be selected at
/// all, so the app can display a number it gives you no way to use. Values are
/// therefore [`widget::selectable_text`] (which brings its own Copy / Select
/// All context menu), with an explicit copy button beside each one for the
/// people who never think to right-click.
pub fn detail<'a>(
    person: &crate::ui::person::Composed<'_>,
    photo: Option<&widget::image::Handle>,
    // Whether a paired phone is in reach, which is what makes texting a
    // number possible at all.
    can_text: bool,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(8).spacing(spacing.space_s);

    // The avatar and the name share the header row. Sized in spacing tokens
    // rather than pixels, per the conventions doc: no raw pixel values.
    column = column.push(
        widget::row::with_capacity(2)
            .align_y(Alignment::Center)
            .spacing(spacing.space_s)
            .push(crate::ui::avatar::avatar(
                photo,
                &person.label,
                f32::from(spacing.space_xxl),
            ))
            .push(widget::text::title3(person.label.clone())),
    );

    if let Some(heading) = &person.organisation {
        column = column.push(
            widget::text::body(heading.clone())
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    if !person.nicknames.is_empty() {
        column = column.push(
            widget::text::caption(format!(
                "\u{201c}{}\u{201d}",
                person.nicknames.join("\u{201d}, \u{201c}")
            ))
            .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    if !person.fields.is_empty() {
        let mut section = widget::settings::section();
        for field in &person.fields {
            section = section.add(value_row(field, can_text));
        }
        column = column.push(section);
    }

    if !person.categories.is_empty() {
        column = column.push(chips(&person.categories));
    }

    // Provenance and honesty: which cards this person is, and what they carry
    // that this app will not touch.
    let mut footer = widget::column::with_capacity(3).spacing(spacing.space_xxs);
    if person.is_linked() {
        // Linked: name every card and offer to take each one back out. One
        // row per card rather than a joined string, because unlinking has to
        // name which card it removes.
        let mut section = widget::settings::section().title(fl!("linked-cards"));
        for (card, book) in person.cards {
            section = section.add(
                widget::settings::item::builder((*book).to_owned())
                    .description(card.label())
                    .control(
                        widget::button::text(fl!("unlink"))
                            .on_press(Message::Unlink(ContactKey::of(card))),
                    ),
            );
        }
        column = column.push(section);
    } else if let Some((_, book)) = person.cards.first() {
        footer = footer.push(
            widget::text::caption(format!("{}: {book}", fl!("in-book")))
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    // The unmodelled properties of every card underneath, so what is being
    // preserved stays visible rather than merely promised.
    let mut unmodelled: Vec<String> = Vec::new();
    for (card, _) in person.cards {
        for property in crate::ui::editor::unmodelled_properties(&card.raw) {
            if !unmodelled.contains(&property) {
                unmodelled.push(property);
            }
        }
    }
    if !unmodelled.is_empty() {
        footer = footer.push(
            widget::text::caption(format!(
                "{}: {}",
                fl!("other-fields"),
                unmodelled.join(", ")
            ))
            .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }
    column = column.push(footer);

    widget::scrollable(column.padding(spacing.space_s))
        .height(Length::Fill)
        .into()
}

/// One labelled, selectable, copyable value, optionally with an action button
/// and the book it came from.
fn value_row<'a>(field: &crate::ui::person::Field<'_>, can_text: bool) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let owned = field.value.clone();

    let mut controls = widget::row::with_capacity(4)
        .align_y(Alignment::Center)
        .spacing(spacing.space_xxs)
        .push(widget::selectable_text::body(owned.clone()));

    let copy: Element<'a, Message> = widget::tooltip(
        widget::button::icon(crate::ui::icon("edit-copy-symbolic"))
            .on_press(Message::Copy(owned)),
        widget::text::body(fl!("copy")),
        widget::tooltip::Position::Top,
    )
    .into();
    controls = controls.push(copy);

    if let Some(url) = &field.action
        && !field.icon.is_empty()
    {
        // The tooltip is this button's only name — an icon alone says
        // nothing to a reader, and "what does this arrow do" is answered by
        // hovering or not at all.
        let action = match field.icon {
            "mail-send-symbolic" => fl!("send-mail"),
            "call-start-symbolic" => fl!("call"),
            _ => fl!("open-link"),
        };
        controls = controls.push(
            widget::tooltip(
                widget::button::icon(crate::ui::icon(field.icon))
                    .on_press(Message::LaunchUrl(url.clone())),
                widget::text::body(action),
                widget::tooltip::Position::Top,
            )
            .apply(Element::from),
        );
    }

    // Texting is offered only when a paired phone is actually in reach: a
    // button that silently does nothing is worse than one that is absent.
    if can_text && let Some(number) = &field.number {
        controls = controls.push(
            widget::tooltip(
                widget::button::icon(crate::ui::icon("mail-message-new-symbolic"))
                    .on_press(Message::SmsRequested(number.clone())),
                widget::text::body(fl!("sms")),
                widget::tooltip::Position::Top,
            )
            .apply(Element::from),
        );
    }

    let mut item = widget::settings::item::builder(field.label.clone());
    // Which card a value came from, on the row itself: a linked person's
    // detail pane is otherwise indistinguishable from one card's, and
    // knowing which server a number will be edited on is the point.
    if let Some(source) = field.source {
        item = item.description(source.to_owned());
    }
    item.control(controls).into()
}

/// Categories as wrapping chips rather than a joined string, so a contact in
/// eight groups does not run off the side of the pane.
fn chips<'a>(categories: &[String]) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let chips: Vec<Element<'a, Message>> = categories
        .iter()
        .map(|category| {
            widget::container(widget::text::caption(category.clone()))
                .padding([spacing.space_xxxs, spacing.space_xxs])
                .class(cosmic::theme::Container::Card)
                .into()
        })
        .collect();

    widget::flex_row(chips)
        .spacing(spacing.space_xxs)
        .row_spacing(spacing.space_xxs)
        .into()
}
