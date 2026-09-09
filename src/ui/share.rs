// SPDX-License-Identifier: GPL-3.0-only

//! Handing a contact to a phone, as a QR code.
//!
//! The one transfer that needs no network, no account, and no cable: point a
//! camera at the screen and the contact is in the other device's address
//! book. Every phone's camera app reads a `BEGIN:VCARD` payload natively.
//!
//! # Why the shared card is not the stored card
//!
//! A QR code holds about 2 900 bytes at the lowest error correction, and a
//! real card with a photo is tens of kilobytes — the encoder would simply
//! refuse. So the payload is built fresh from the fields a phone actually
//! files: name, numbers, addresses, organisation, and websites. No photo, no
//! notes, no `X-` properties. The card on disk is untouched and unabridged;
//! this is a transfer format, and pretending otherwise would produce a code
//! nothing can scan.
//!
//! vCard **3.0** regardless of the setting, because it is what phone cameras
//! and every contacts app read without complaint. A 4.0 payload is legal and
//! less widely understood, and this is the one place where the receiving end
//! is a stranger's device.

use cosmic::Element;
use cosmic::iced::{Alignment, Length};
use cosmic::widget;

use crate::fl;
use crate::ui::person::Composed;

/// The most bytes a QR code can carry in byte mode at the lowest error
/// correction — version 40, level L. Past this the encoder fails, so the
/// payload is trimmed before it gets there.
const QR_CAPACITY: usize = 2_953;

/// The card a QR code carries: enough to file the person, small enough to
/// scan.
#[must_use]
pub fn vcard(person: &Composed<'_>) -> String {
    let head = person.head;
    let mut out = String::from("BEGIN:VCARD\r\nVERSION:3.0\r\n");

    out.push_str(&line("FN", &person.label));
    // `N` is required by 3.0 and is what a phone splits into first and last
    // name fields; a card without it files under one long string.
    out.push_str(&format!(
        "N:{};{};{};{};{}\r\n",
        escape(&head.name.family),
        escape(&head.name.given),
        escape(&head.name.additional),
        escape(&head.name.prefix),
        escape(&head.name.suffix),
    ));

    for field in &person.fields {
        match field.icon {
            "mail-send-symbolic" => out.push_str(&line("EMAIL;TYPE=INTERNET", &field.value)),
            "call-start-symbolic" => out.push_str(&line("TEL", &field.value)),
            "web-browser-symbolic" => out.push_str(&line("URL", &field.value)),
            _ => {}
        }
    }

    // Addresses come through the composed fields as one line each, which is
    // not a structured `ADR`; take them from the head card, where they are
    // still in parts.
    for address in &head.addresses {
        out.push_str(&format!(
            "ADR;TYPE=HOME:{};{};{};{};{};{};{}\r\n",
            escape(&address.po_box),
            escape(&address.extended),
            escape(&address.street),
            escape(&address.locality),
            escape(&address.region),
            escape(&address.postal_code),
            escape(&address.country),
        ));
    }

    if let Some(organisation) = &person.organisation {
        out.push_str(&line("ORG", organisation));
    }

    out.push_str("END:VCARD\r\n");
    out
}

/// `PROPERTY:value`, escaped and folded the way RFC 2426 asks.
fn line(property: &str, value: &str) -> String {
    format!("{property}:{}\r\n", escape(value))
}

/// vCard's own escaping: backslash, comma, semicolon, and newline.
///
/// Missing this is how a contact called `Lovelace, Ada` becomes two values
/// on the receiving phone.
fn escape(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace(';', "\\;")
        .replace(',', "\\,")
        .replace('\n', "\\n")
        .replace('\r', "")
}

/// The QR code as an SVG document, or `None` when the payload will not fit
/// even after trimming.
#[must_use]
pub fn qr_svg(payload: &str) -> Option<String> {
    if payload.len() > QR_CAPACITY {
        return None;
    }
    let code = qrcode::QrCode::new(payload.as_bytes()).ok()?;
    let width = code.width();
    let colors = code.to_colors();

    // A quiet zone of four modules is part of the specification, not padding:
    // scanners use it to find the code's edge, and a QR drawn flush to its
    // container reads unreliably.
    const QUIET: usize = 4;
    let side = width + QUIET * 2;

    // One path of many rectangles rather than one element per module: a
    // version-40 code is 177² modules, and 31 000 SVG elements is a renderer
    // problem where one path is not.
    let mut path = String::new();
    for (index, color) in colors.iter().enumerate() {
        if *color == qrcode::Color::Dark {
            let (x, y) = (index % width + QUIET, index / width + QUIET);
            path.push_str(&format!("M{x} {y}h1v1h-1z"));
        }
    }

    Some(format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {side} {side}\">\
         <rect width=\"{side}\" height=\"{side}\" fill=\"#fff\"/>\
         <path d=\"{path}\" fill=\"#000\"/></svg>"
    ))
}

/// The share dialog's contents: the code, who it is, and the warning when it
/// could not be made.
pub fn view<'a, M: 'static>(person: &Composed<'_>) -> Element<'a, M> {
    let spacing = cosmic::theme::spacing();
    let payload = vcard(person);

    let body: Element<'a, M> = match qr_svg(&payload) {
        Some(svg) => widget::svg(widget::svg::Handle::from_memory(svg.into_bytes()))
            .width(Length::Fixed(f32::from(spacing.space_xxl) * 5.0))
            .height(Length::Fixed(f32::from(spacing.space_xxl) * 5.0))
            .into(),
        // Only reachable for a contact with an implausible number of fields;
        // saying so beats an empty box.
        None => widget::text::body(fl!("share-too-big"))
            .wrapping(cosmic::iced::core::text::Wrapping::Word)
            .into(),
    };

    widget::column::with_capacity(3)
        .align_x(Alignment::Center)
        .spacing(spacing.space_s)
        .push(body)
        .push(
            widget::text::caption(fl!("share-hint"))
                .wrapping(cosmic::iced::core::text::Wrapping::Word),
        )
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_pim_core::model::{Contact, Typed};

    fn typed(value: &str) -> Typed {
        Typed {
            value: value.into(),
            types: Vec::new(),
            pref: None,
            group: None,
            params: Vec::new(),
        }
    }

    fn ada() -> Contact {
        let mut c = Contact::draft("personal");
        c.uid = "ada".into();
        c.display_name = "Ada Lovelace".into();
        c.name.given = "Ada".into();
        c.name.family = "Lovelace".into();
        c.emails.push(typed("ada@example.org"));
        c.phones.push(typed("+30 210 1234567"));
        c
    }

    fn shared(card: &Contact) -> String {
        let cards = [(card, "Personal")];
        vcard(&crate::ui::person::compose(&cards).expect("compose"))
    }

    #[test]
    fn the_payload_is_a_scannable_three_oh_card() {
        let text = shared(&ada());
        assert!(text.starts_with("BEGIN:VCARD\r\nVERSION:3.0\r\n"));
        assert!(text.ends_with("END:VCARD\r\n"));
        assert!(text.contains("FN:Ada Lovelace\r\n"));
        assert!(text.contains("N:Lovelace;Ada;;;\r\n"));
        assert!(text.contains("TEL:+30 210 1234567\r\n"));
        assert!(text.contains("EMAIL;TYPE=INTERNET:ada@example.org\r\n"));
    }

    /// The whole reason the payload is rebuilt rather than copied.
    #[test]
    fn a_photo_never_reaches_the_code() {
        let mut card = ada();
        card.has_photo = true;
        card.raw = format!("PHOTO;ENCODING=b:{}", "A".repeat(40_000));
        let text = shared(&card);
        assert!(!text.contains("PHOTO"));
        assert!(qr_svg(&text).is_some(), "a normal contact would not encode");
    }

    /// A comma in a name is one value, not two, on the receiving phone.
    #[test]
    fn separators_in_values_are_escaped() {
        let mut card = ada();
        card.display_name = "Lovelace, Ada; Countess".into();
        let text = shared(&card);
        assert!(text.contains(r"FN:Lovelace\, Ada\; Countess"), "{text}");
    }

    #[test]
    fn a_linked_persons_numbers_all_travel() {
        let mut work = ada();
        work.addressbook_id = "work".into();
        let mut home = ada();
        home.addressbook_id = "personal".into();
        home.uid = "ada-home".into();
        home.phones = vec![typed("+30 694 7654321")];
        home.emails.clear();

        let cards = [(&work, "Work"), (&home, "Personal")];
        let text = vcard(&crate::ui::person::compose(&cards).expect("compose"));
        assert!(text.contains("TEL:+30 210 1234567\r\n"));
        assert!(
            text.contains("TEL:+30 694 7654321\r\n"),
            "the second card's number did not travel: {text}"
        );
    }

    /// Scanners find a code by its four-module margin; one drawn flush to its
    /// container reads unreliably or not at all.
    #[test]
    fn the_code_carries_a_quiet_zone() {
        let svg = qr_svg("BEGIN:VCARD\r\nVERSION:3.0\r\nFN:A\r\nEND:VCARD\r\n").expect("encode");
        assert!(svg.starts_with("<svg"));

        let side: usize = svg
            .split("viewBox=\"0 0 ")
            .nth(1)
            .and_then(|rest| rest.split(' ').next())
            .and_then(|n| n.parse().ok())
            .unwrap_or_else(|| panic!("no viewBox in {}", &svg[..80]));

        // QR sizes run 21, 25, 29 … so the drawn side is one of those plus the
        // eight modules of margin.
        let modules = side.checked_sub(8).expect("a code smaller than its margin");
        assert!(
            modules >= 21 && (modules - 21).is_multiple_of(4),
            "{modules} is not a QR size, so the margin is wrong"
        );

        // Nothing is drawn inside the margin.
        assert!(
            !svg.contains("M0 ") && !svg.contains("M1 ") && !svg.contains("M2 "),
            "a module was drawn inside the quiet zone"
        );
    }

    #[test]
    fn an_oversized_payload_is_refused_rather_than_drawn_wrong() {
        assert!(qr_svg(&"x".repeat(QR_CAPACITY + 1)).is_none());
    }
}
