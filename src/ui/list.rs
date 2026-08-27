// SPDX-License-Identifier: GPL-3.0-only

//! The contact list and the detail pane.

use cosmic::Element;
use cosmic::iced::core::text::{Ellipsize, EllipsizeHeightLimit};
use cosmic::iced::{Alignment, Length};
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
        content = content.push(widget::icon::from_name(if ticked {
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

/// The detail pane for one contact.
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
    contact: &'a Contact,
    book_name: Option<&'a str>,
    photo: Option<&'a widget::image::Handle>,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(8).spacing(spacing.space_s);

    // The avatar and the name share the header row. Sized in spacing tokens
    // rather than pixels, per the conventions doc: no raw pixel values.
    let name = widget::text::title3(contact.label());
    column = column.push(
        widget::row::with_capacity(2)
            .align_y(Alignment::Center)
            .spacing(spacing.space_s)
            .push(crate::ui::avatar::avatar(
                photo,
                &contact.label(),
                f32::from(spacing.space_xxl),
            ))
            .push(name),
    );

    if let Some(org) = &contact.organisation {
        let heading = match &contact.title {
            Some(title) if !title.trim().is_empty() => format!("{title}, {org}"),
            _ => org.clone(),
        };
        column = column.push(
            widget::text::body(heading).class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    if !contact.nicknames.is_empty() {
        column = column.push(
            widget::text::caption(format!("“{}”", contact.nicknames.join("”, “")))
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }

    let mut section = widget::settings::section();
    let mut any = false;

    for email in &contact.emails {
        any = true;
        section = section.add(value_row(
            email.label().unwrap_or("email").to_owned(),
            &email.value,
            Some(format!("mailto:{}", email.value)),
            "mail-send-symbolic",
        ));
    }
    for phone in &contact.phones {
        any = true;
        section = section.add(value_row(
            phone.label().unwrap_or("phone").to_owned(),
            &phone.value,
            // `tel:` is handed to the desktop's handler. Without one nothing
            // happens, which is why the value stays copyable regardless.
            Some(format!("tel:{}", phone.value.replace(' ', ""))),
            "call-start-symbolic",
        ));
    }
    for address in &contact.addresses {
        any = true;
        section = section.add(value_row(
            address
                .types
                .first()
                .cloned()
                .unwrap_or_else(|| fl!("address")),
            &address.one_line(),
            None,
            "",
        ));
    }
    for url in &contact.urls {
        any = true;
        section = section.add(value_row(
            url.label().unwrap_or("website").to_owned(),
            &url.value,
            Some(url.value.clone()),
            "web-browser-symbolic",
        ));
    }
    if let Some(birthday) = contact.birthday {
        any = true;
        section = section.add(value_row(
            fl!("birthday"),
            &birthday.format("%-d %B %Y").to_string(),
            None,
            "",
        ));
    }
    if let Some(note) = &contact.note {
        any = true;
        section = section.add(value_row(fl!("note"), note, None, ""));
    }

    if any {
        column = column.push(section);
    }

    if !contact.categories.is_empty() {
        column = column.push(chips(&contact.categories));
    }

    // Provenance and honesty: which book this came from, and what the card
    // carries that this app will not touch.
    let mut footer = widget::column::with_capacity(2).spacing(spacing.space_xxs);
    if let Some(name) = book_name {
        footer = footer.push(
            widget::text::caption(format!("{}: {name}", fl!("in-book")))
                .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
        );
    }
    let unmodelled = crate::ui::editor::unmodelled_properties(&contact.raw);
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

/// One labelled, selectable, copyable value, optionally with an action button.
fn value_row<'a>(
    label: String,
    value: &str,
    action: Option<String>,
    action_icon: &'a str,
) -> Element<'a, Message> {
    let spacing = cosmic::theme::spacing();
    let owned = value.to_owned();

    let mut controls = widget::row::with_capacity(3)
        .align_y(Alignment::Center)
        .spacing(spacing.space_xxs)
        .push(widget::selectable_text::body(owned.clone()));

    let copy: Element<'a, Message> = widget::tooltip(
        widget::button::icon(widget::icon::from_name("edit-copy-symbolic"))
            .on_press(Message::Copy(owned)),
        widget::text::body(fl!("copy")),
        widget::tooltip::Position::Top,
    )
    .into();
    controls = controls.push(copy);

    if let Some(url) = action
        && !action_icon.is_empty()
    {
        controls = controls.push(
            widget::button::icon(widget::icon::from_name(action_icon))
                .on_press(Message::LaunchUrl(url)),
        );
    }

    widget::settings::item::builder(label)
        .control(controls)
        .into()
}

/// Categories as wrapping chips rather than a joined string, so a contact in
/// eight groups does not run off the side of the pane.
fn chips(categories: &[String]) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let chips: Vec<Element<'_, Message>> = categories
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
