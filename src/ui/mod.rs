// SPDX-License-Identifier: GPL-3.0-only

//! Widgets and view helpers.

pub mod accounts;
pub mod avatar;
pub mod csv;
pub mod editor;
pub mod list;
pub mod person;
pub mod review;
pub mod share;

/// Dimmed secondary text, matching the rest of the suite.
#[must_use]
pub fn dim_text(theme: &cosmic::Theme) -> cosmic::iced::widget::text::Style {
    let mut color = theme.cosmic().on_bg_color();
    color.alpha *= 0.7;
    cosmic::iced::widget::text::Style {
        color: Some(color.into()),
        ..Default::default()
    }
}
