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
//! # Why the payload is not folded
//!
//! RFC 2426 says a line SHOULD be folded at 75 octets, and this does not fold
//! at all. Deliberate, for two reasons that only apply here: a fold costs
//! three bytes and the capacity is 2 331, so folding spends the budget that
//! decides whether a contact fits at all; and the readers are phone cameras,
//! which handle unfolded lines universally. SHOULD, not MUST.
//!
//! It is written down because an undecided-looking absence and a decision
//! look identical in code. `a_greek_contact_with_long_values_parses_back_whole`
//! is the check that this stays true for values long enough to want folding,
//! in a script where octets and characters differ.
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

/// The most bytes this encoder accepts.
///
/// **Measured, not reasoned.** The specification's byte-mode maximum is 2 953
/// — version 40 at error correction L — and that number is wrong here,
/// because `QrCode::new` chooses level M, whose maximum is 2 331. Taking the
/// figure from the specification let a payload between the two pass this
/// guard and then fail to encode, turning a card that should have been
/// refused with a reason into a `None` the caller reported as "too much in
/// it" anyway. Right answer, wrong route.
///
/// `the_capacity_constant_matches_what_the_encoder_accepts` finds the real
/// limit by bisection, so this cannot drift if the crate changes its default.
const QR_CAPACITY: usize = 2_331;

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

    // ORG carries the company and its department levels as separate
    // components, and TITLE is its own property — never folded into ORG,
    // which would file the contact under a company that does not exist.
    if let Some(organisation) = &person.organisation {
        let mut value = escape(organisation);
        for unit in &person.organisation_units {
            value.push(';');
            value.push_str(&escape(unit));
        }
        out.push_str(&format!("ORG:{value}\r\n"));
    }
    if let Some(title) = &person.title {
        out.push_str(&line("TITLE", title));
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
/// Missing this is how a contact called `Lovelace, Ada` becomes two values on
/// the receiving phone.
///
/// The substrate's, not a second copy. It made this public for exactly this
/// reason — its own writeback patcher must escape identically — and a rule
/// with two implementations in one program has two places to drift, which is
/// the drift nobody notices because both look right in isolation.
use cosmic_pim_core::ical::escape_text as escape;

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

    /// The payload has to *parse*, not merely look right.
    ///
    /// Every other assertion here is `contains` on text this module wrote,
    /// which cannot tell a scannable card from a plausible-looking one. A QR
    /// code no parser reads is worse than no QR code, because the whole point
    /// is that a stranger's phone reads it. So this puts the payload back
    /// through a real parser and checks the values arrive.
    #[test]
    fn the_payload_parses_back_into_the_contact_it_came_from() {
        let mut card = ada();
        card.organisation = Some("Analytical Engine Co".into());
        card.organisation_units = vec!["Research".into(), "Difference Engines".into()];
        card.title = Some("Mathematician".into());
        card.urls.push(typed("https://example.org/ada"));

        let cards = [(&card, "Personal")];
        let payload = vcard(&crate::ui::person::compose(&cards).expect("compose"));

        let back = cosmic_pim_core::vcard::parse_vcards(&payload, "book", "x.vcf")
            .pop()
            .expect("the payload parses as a vCard");

        assert_eq!(back.label(), "Ada Lovelace");
        assert_eq!(back.name.family, "Lovelace");
        assert_eq!(back.name.given, "Ada");
        assert_eq!(
            back.emails.first().map(|e| e.value.as_str()),
            Some("ada@example.org")
        );
        assert_eq!(
            back.phones.first().map(|p| p.value.as_str()),
            Some("+30 210 1234567")
        );
        assert_eq!(
            back.urls.first().map(|u| u.value.as_str()),
            Some("https://example.org/ada")
        );

        // The regression this test was written for: the composed *heading* —
        // "Mathematician, Analytical Engine Co ‣ Research" — was being written
        // into ORG, so a receiving phone filed the contact under a company by
        // that name. ORG carries components; TITLE is its own property.
        assert_eq!(
            back.organisation.as_deref(),
            Some("Analytical Engine Co"),
            "a display heading was written into ORG"
        );
        assert_eq!(
            back.organisation_units,
            vec!["Research", "Difference Engines"]
        );
        assert_eq!(back.title.as_deref(), Some("Mathematician"));
    }

    /// A name with separators in it has to come back as one name, not several
    /// fields — the escaping has to survive a real parser, not just look
    /// escaped.
    #[test]
    fn an_escaped_name_parses_back_whole() {
        let mut card = ada();
        card.display_name = "Lovelace, Ada; Countess".into();

        let cards = [(&card, "Personal")];
        let payload = vcard(&crate::ui::person::compose(&cards).expect("compose"));
        let back = cosmic_pim_core::vcard::parse_vcards(&payload, "book", "x.vcf")
            .pop()
            .expect("parses");

        assert_eq!(back.label(), "Lovelace, Ada; Countess");
    }

    /// Every other test here feeds short ASCII, which cannot reach two
    /// properties they are named for: that the escaping survives a multibyte
    /// value, and that a value long enough to want folding still parses.
    ///
    /// This project's user writes Greek. A test that can only exercise a
    /// property with input it never supplies is a test that names the right
    /// thing and cannot see it.
    /// Every character the escaper treats specially, through a real parser.
    ///
    /// The escaping is the substrate's now rather than a second copy, and the
    /// point of a shared rule is that a change to it surfaces in every caller
    /// — which it can only do if each caller covers the rule rather than
    /// assuming it.
    #[test]
    fn every_escaped_character_survives_a_round_trip() {
        let awkward = "back\\slash semi;colon comma,here";
        let mut card = ada();
        card.display_name = awkward.into();
        card.name.family = "semi;colon".into();

        let cards = [(&card, "Personal")];
        let payload = vcard(&crate::ui::person::compose(&cards).expect("compose"));
        let back = cosmic_pim_core::vcard::parse_vcards(&payload, "book", "x.vcf")
            .pop()
            .expect("parses");

        assert_eq!(
            back.label(),
            awkward,
            "an escaped character did not survive"
        );
        assert_eq!(
            back.name.family, "semi;colon",
            "a semicolon inside a name component split it into two components"
        );
    }

    /// A carriage return is dropped rather than escaped, so a value that
    /// arrived with CRLF in it cannot inject a line break into the payload.
    #[test]
    fn a_value_containing_a_line_break_cannot_break_the_payload() {
        let mut card = ada();
        card.display_name = "Ada\r\nEMAIL:injected@example.org".into();

        let cards = [(&card, "Personal")];
        let payload = vcard(&crate::ui::person::compose(&cards).expect("compose"));
        let back = cosmic_pim_core::vcard::parse_vcards(&payload, "book", "x.vcf")
            .pop()
            .expect("parses");

        assert_eq!(
            back.emails.len(),
            1,
            "a line break in a value forged a property: {payload}"
        );
        assert_eq!(back.emails[0].value, "ada@example.org");
    }

    #[test]
    fn a_greek_contact_with_long_values_parses_back_whole() {
        let name = "\u{393}\u{3b9}\u{3ce}\u{3c1}\u{3b3}\u{3bf}\u{3c2} \u{3a0}\u{3b1}\u{3c0}\u{3b1}\u{3b4}\u{3cc}\u{3c0}\u{3bf}\u{3c5}\u{3bb}\u{3bf}\u{3c2}";
        // Comfortably past the 75 octets RFC 2426 folds at, and in a script
        // where octets and characters differ.
        let street = "\u{39b}\u{3b5}\u{3c9}\u{3c6}\u{3cc}\u{3c1}\u{3bf}\u{3c5} \u{39a}\u{3b1}\u{3bb}\u{3bb}\u{3b9}\u{3b3}\u{3ac} 128, \u{386}\u{3bd}\u{3c9} \u{393}\u{3bb}\u{3c5}\u{3c6}\u{3ac}\u{3b4}\u{3b1}, \u{391}\u{3b8}\u{3ae}\u{3bd}\u{3b1}";

        let mut card = ada();
        card.display_name = name.into();
        card.name.family =
            "\u{3a0}\u{3b1}\u{3c0}\u{3b1}\u{3b4}\u{3cc}\u{3c0}\u{3bf}\u{3c5}\u{3bb}\u{3bf}\u{3c2}"
                .into();
        card.name.given = "\u{393}\u{3b9}\u{3ce}\u{3c1}\u{3b3}\u{3bf}\u{3c2}".into();
        card.addresses.push(cosmic_pim_core::model::Address {
            street: street.into(),
            locality: "\u{391}\u{3b8}\u{3ae}\u{3bd}\u{3b1}".into(),
            country: "\u{395}\u{3bb}\u{3bb}\u{3ac}\u{3b4}\u{3b1}".into(),
            ..Default::default()
        });

        let cards = [(&card, "Personal")];
        let composed = crate::ui::person::compose(&cards).expect("compose");
        let payload = vcard(&composed);

        assert!(
            payload.len() > street.len(),
            "the address never reached the payload"
        );

        let back = cosmic_pim_core::vcard::parse_vcards(&payload, "book", "x.vcf")
            .pop()
            .expect("a Greek payload parses as a vCard");

        assert_eq!(
            back.label(),
            name,
            "the name did not survive the round trip"
        );
        assert_eq!(
            back.name.given,
            "\u{393}\u{3b9}\u{3ce}\u{3c1}\u{3b3}\u{3bf}\u{3c2}"
        );
        assert_eq!(
            back.addresses.first().map(|a| a.street.as_str()),
            Some(street),
            "a long multibyte street did not survive"
        );

        // And it still fits a code, which is the only reason the payload is
        // rebuilt rather than copied.
        assert!(
            qr_svg(&payload).is_some(),
            "a Greek contact of ordinary size does not fit in a QR code"
        );
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

#[cfg(test)]
mod svg_tests {
    use super::*;

    /// The SVG is assembled by hand, so it is checked with the parser that
    /// actually renders it.
    ///
    /// Every other assertion about it — the viewBox, the quiet zone — is a
    /// `contains` on a string this module had just built, and none of them can
    /// tell a drawable document from a plausible-looking one. The path data is
    /// concatenated from thousands of `M x y h1v1h-1z` fragments; one bad
    /// coordinate makes a code that renders blank, and blank is exactly what a
    /// user cannot distinguish from "the camera did not pick it up".
    ///
    /// `usvg` is the parser underneath the widget this SVG is handed to, so
    /// this is the consumer's own reader rather than a second opinion.
    fn parse(svg: &str) -> usvg::Tree {
        usvg::Tree::from_str(svg, &usvg::Options::default())
            .unwrap_or_else(|why| panic!("the renderer cannot read this SVG: {why}\n{svg}"))
    }

    fn payload() -> String {
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\n\
         EMAIL;TYPE=INTERNET:ada@example.org\r\nEND:VCARD\r\n"
            .to_owned()
    }

    #[test]
    fn the_code_is_an_svg_the_renderer_can_read() {
        let svg = qr_svg(&payload()).expect("encode");
        let tree = parse(&svg);

        let size = tree.size();
        assert!(size.width() > 0.0 && size.height() > 0.0, "{size:?}");
        assert!(
            (size.width() - size.height()).abs() < f32::EPSILON,
            "a QR code must be square, got {size:?}"
        );
    }

    /// A document that parses but draws nothing is the failure this is really
    /// guarding: the modules are one long path, and an unparseable `d`
    /// attribute is dropped rather than rejected.
    #[test]
    fn the_modules_survive_as_actual_geometry() {
        let svg = qr_svg(&payload()).expect("encode");
        let tree = parse(&svg);

        let paths = tree
            .root()
            .children()
            .iter()
            .filter(|node| matches!(node, usvg::Node::Path(_)))
            .count();
        assert!(
            paths >= 2,
            "expected the background and the modules to survive parsing, got {paths} paths"
        );
    }

    /// The smallest and largest codes exercise different geometry, and the
    /// largest is where a concatenated path is most likely to go wrong.
    /// Finds the largest payload `QrCode::new` actually accepts, which is
    /// the number the guard has to agree with.
    #[test]
    fn the_capacity_constant_matches_what_the_encoder_accepts() {
        let accepts = |n: usize| qrcode::QrCode::new("x".repeat(n).as_bytes()).is_ok();

        let (mut lo, mut hi) = (1usize, 4096usize);
        while lo < hi {
            let mid = lo.midpoint(hi + 1);
            if accepts(mid) { lo = mid } else { hi = mid - 1 }
        }
        assert_eq!(
            QR_CAPACITY, lo,
            "QR_CAPACITY says {QR_CAPACITY} but the encoder accepts at most {lo}; \
             a payload between them passes the guard and then fails to encode"
        );
    }

    #[test]
    fn codes_of_every_size_parse() {
        for length in [1usize, 100, 1000, QR_CAPACITY] {
            let svg = qr_svg(&"x".repeat(length)).expect("encode");
            let tree = parse(&svg);
            assert!(
                tree.size().width() > 0.0,
                "a {length}-byte payload drew nothing"
            );
        }
    }
}
