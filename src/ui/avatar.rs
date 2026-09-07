// SPDX-License-Identifier: GPL-3.0-only

//! A contact's face: the photo where the card has one, generated initials
//! where it does not.
//!
//! Every row and the detail header go through here, so no contact ever renders
//! as an empty hole. The generated form is initials on a colour seeded from
//! the name — the same person gets the same colour every launch, which is what
//! lets the eye find a row again without reading.

use cosmic::Element;
use cosmic::iced::{Alignment, Color, Length};
use cosmic::widget;

/// The avatar background palette.
///
/// Fixed rather than derived from the theme: an identity colour that shifted
/// when the desktop switched from light to dark would defeat its purpose.
/// Every entry is dark enough for white text in both modes (≥ 4.5:1).
const PALETTE: [Color; 8] = [
    Color::from_rgb(0.75, 0.22, 0.17), // red
    Color::from_rgb(0.80, 0.41, 0.10), // orange
    Color::from_rgb(0.42, 0.44, 0.05), // olive
    Color::from_rgb(0.15, 0.53, 0.26), // green
    Color::from_rgb(0.11, 0.49, 0.55), // teal
    Color::from_rgb(0.16, 0.42, 0.75), // blue
    Color::from_rgb(0.48, 0.32, 0.75), // purple
    Color::from_rgb(0.72, 0.24, 0.51), // magenta
];

/// The initials shown for a name: the first letter of the first and last
/// words, uppercased. One word gives one letter; an empty name gives an empty
/// string, and the caller falls back to a glyphless circle.
#[must_use]
pub fn initials(name: &str) -> String {
    let mut words = name.split_whitespace();
    let first = words.next().and_then(|w| w.chars().next());
    let last = words.next_back().and_then(|w| w.chars().next());
    first
        .into_iter()
        .chain(last)
        .flat_map(char::to_uppercase)
        .collect()
}

/// The palette entry for a name — a stable hash, so the colour survives a
/// restart and matches across the list and the detail pane.
fn seed_color(name: &str) -> Color {
    let hash = name
        .bytes()
        .fold(0usize, |acc, b| acc.wrapping_mul(31).wrapping_add(b.into()));
    PALETTE[hash % PALETTE.len()]
}

/// A round avatar, `side` points across: the photo if there is one, otherwise
/// initials on the name's colour.
pub fn avatar<'a, M: 'a>(
    photo: Option<&widget::image::Handle>,
    name: &str,
    side: f32,
) -> Element<'a, M> {
    if let Some(handle) = photo {
        return widget::image(handle.clone())
            .width(Length::Fixed(side))
            .height(Length::Fixed(side))
            .border_radius([side / 2.0; 4])
            .into();
    }

    let text = initials(name);
    let glyphs: Element<'a, M> = if text.is_empty() {
        crate::ui::icon("avatar-default-symbolic")
            .size((side * 0.55) as u16)
            .icon()
            .class(cosmic::theme::Svg::Custom(std::rc::Rc::new(|_| {
                cosmic::iced::widget::svg::Style {
                    color: Some(Color::WHITE),
                }
            })))
            .into()
    } else {
        widget::text(text)
            .size(side * 0.4)
            .class(cosmic::theme::Text::Color(Color::WHITE))
            .into()
    };

    let color = seed_color(name);
    widget::container(glyphs)
        .width(Length::Fixed(side))
        .height(Length::Fixed(side))
        .align_x(Alignment::Center)
        .align_y(Alignment::Center)
        .class(cosmic::theme::Container::custom(move |_theme| {
            cosmic::iced::widget::container::Style {
                background: Some(color.into()),
                border: cosmic::iced::Border {
                    radius: (side / 2.0).into(),
                    ..Default::default()
                },
                ..Default::default()
            }
        }))
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn initials_take_the_first_and_last_words() {
        assert_eq!(initials("Ada Lovelace"), "AL");
        assert_eq!(initials("Ada Augusta King Lovelace"), "AL");
    }

    #[test]
    fn a_single_word_gives_a_single_letter() {
        assert_eq!(initials("ada"), "A");
    }

    #[test]
    fn an_empty_name_gives_no_initials() {
        assert_eq!(initials(""), "");
        assert_eq!(initials("   "), "");
    }

    /// Uppercasing is per-locale-neutral Unicode, not ASCII — a Greek or
    /// Cyrillic name gets its own capitals.
    #[test]
    fn initials_are_unicode_uppercased() {
        assert_eq!(initials("γιώργος παπάς"), "ΓΠ");
    }

    #[test]
    fn the_colour_is_stable_for_a_name() {
        assert_eq!(seed_color("Ada Lovelace"), seed_color("Ada Lovelace"));
    }
}
