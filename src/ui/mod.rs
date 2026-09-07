// SPDX-License-Identifier: GPL-3.0-only

//! Widgets and view helpers.

pub mod accounts;
pub mod avatar;
pub mod csv;
pub mod dialogs;
pub mod editor;
pub mod list;
pub mod menus;
pub mod person;
pub mod review;
pub mod settings;
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

/// A named icon with an explicit fallback chain.
///
/// libcosmic's default fallback truncates the name at each `-` and retries, so
/// a missing `mail-message-new-symbolic` degrades to `mail-message-new`, then
/// `mail-message`, then `mail`. That is plausible for some names and lands on
/// nothing for others — `object-select-symbolic` truncates to `object`, which
/// no theme ships — and a lookup that finds nothing renders a **blank** icon
/// with no warning at all. On the machine this was written on every name below
/// resolves; the chain is for the COSMIC installs where one does not.
///
/// Naming a replacement per icon rather than one generic fallback is the whole
/// point: the replacement has to still mean something in that button.
pub fn icon(name: &'static str) -> cosmic::widget::icon::Named {
    use cosmic::widget::icon::IconFallback;

    let fallbacks = fallbacks(name);
    let named = cosmic::widget::icon::from_name(name).prefer_svg(true);
    if fallbacks.is_empty() {
        named
    } else {
        named.fallback(Some(IconFallback::Names(
            fallbacks.iter().map(|name| (*name).into()).collect(),
        )))
    }
}

/// The replacement names for one icon, most specific first.
///
/// Each entry degrades toward a name from a broader, older set — the
/// freedesktop standard names, which every theme in the COSMIC → Pop →
/// hicolor chain carries.
fn fallbacks(name: &str) -> &'static [&'static str] {
    match name {
        "avatar-default-symbolic" => &["avatar-default", "user-info-symbolic", "user-info"],
        "call-start-symbolic" => &["call-start", "phone-symbolic", "phone"],
        "checkbox-checked-symbolic" => &["checkbox-checked", "object-select-symbolic"],
        "checkbox-symbolic" => &["checkbox", "list-add-symbolic"],
        "dialog-question-symbolic" => &["dialog-question", "help-about-symbolic"],
        "edit-copy-symbolic" => &["edit-copy", "gtk-copy"],
        "folder-symbolic" => &["folder", "inode-directory"],
        "go-previous-symbolic" => &["go-previous", "gtk-go-back"],
        "list-remove-symbolic" => &["list-remove", "edit-delete-symbolic", "gtk-remove"],
        "mail-message-new-symbolic" => &["mail-message-new", "mail-send-symbolic", "mail-send"],
        "mail-send-symbolic" => &["mail-send", "mail-message-new-symbolic", "mail-unread"],
        "non-starred-symbolic" => &["non-starred", "starred-symbolic"],
        "object-select-symbolic" => &["object-select", "gtk-apply", "emblem-ok-symbolic"],
        "starred-symbolic" => &["starred", "bookmark-new-symbolic"],
        "system-users-symbolic" => &["system-users", "avatar-default-symbolic", "user-info"],
        "web-browser-symbolic" => &["web-browser", "applications-internet"],
        // An unlisted name keeps libcosmic's truncating default, which is
        // better than an empty chain: it at least tries something. The test
        // below is what stops a new icon quietly landing here.
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::fallbacks;

    /// Every icon this application asks for by name must name its own
    /// replacement.
    ///
    /// Read off the source rather than a hand-kept list, because a hand-kept
    /// list is exactly what drifts: a new lookup added anywhere fails this
    /// until its chain exists. A missing chain is not a crash — it is a blank
    /// button on somebody else's icon theme, which is why nothing else would
    /// catch it.
    ///
    /// The scan reads this file too, so nothing here may spell the call it
    /// looks for; that is why the sentence above describes it instead.
    #[test]
    fn every_icon_used_in_the_source_has_a_fallback_chain() {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut unchained: Vec<String> = Vec::new();
        let mut checked = 0usize;

        let mut stack = vec![root];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).expect("readable source directory") {
                let path = entry.expect("readable entry").path();
                if path.is_dir() {
                    stack.push(path);
                    continue;
                }
                if path.extension().is_none_or(|e| e != "rs") {
                    continue;
                }
                // This file defines the table and never calls the helper, but
                // it does spell the call — in the split pattern below and in
                // the comment above it — so scanning it would match the
                // scanner's own source and fail on a name nobody uses.
                if path.ends_with("ui/mod.rs") {
                    continue;
                }
                let source = std::fs::read_to_string(&path).expect("readable source file");
                // A lookup by string literal, allowing the newline the
                // formatter inserts for a long call.
                for rest in source.split("crate::ui::icon(").skip(1) {
                    let trimmed = rest.trim_start();
                    let Some(quoted) = trimmed.strip_prefix('"') else {
                        continue; // A dynamic name; its literals are found elsewhere.
                    };
                    let Some(name) = quoted.split('"').next() else {
                        continue;
                    };
                    checked += 1;
                    if fallbacks(name).is_empty() {
                        unchained.push(format!("{name} (in {})", path.display()));
                    }
                }
            }
        }

        assert!(
            checked > 0,
            "found no icon lookups to check; the scan is broken"
        );
        assert!(
            unchained.is_empty(),
            "these icons would render blank on a theme that lacks them, with no \
             warning — give each a fallback chain in `fallbacks`: {unchained:#?}"
        );
    }
}
