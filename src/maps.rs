// SPDX-License-Identifier: GPL-3.0-only

//! Showing an address in the desktop's maps application.
//!
//! GNOME Contacts 51 does this with a "Show on the map" button beside each
//! address, offered only when something handles the `maps:` scheme, and
//! launches `maps:q=<address>` with the address percent-encoded
//! (`contacts-contact-sheet.vala`, `Address.to_maps_uri`). Only GNOME Maps
//! handles `maps:`, so Circle asks for the standard scheme instead: RFC 5870
//! `geo:`, with the address as a free-text `q` query at `0,0` — "no location,
//! search for this". GNOME Maps reads that form as a search with no location
//! bias, and KDE's geo handlers (OpenStreetMap and friends, often installed on
//! a COSMIC desktop through KDE libraries) turn it into a web map search.
//!
//! The URI is handed to the desktop's opener like every other link in the
//! detail pane, and the button is offered only when some installed
//! application declares `x-scheme-handler/geo` — a button that opens nothing
//! is worse than none, the same rule texting follows.

use std::fmt::Write as _;

/// The icon on the button.
pub const ICON: &str = "mark-location-symbolic";

/// The `geo:` URI that searches for `address`.
///
/// Every byte outside RFC 3986's unreserved set is percent-encoded — what
/// GNOME's `GLib.Uri.escape_string` does with no reserved characters allowed.
/// So an address holding `&`, `#` or `=` stays one query value rather than
/// ending it or starting another parameter, and a Greek street name travels as
/// its UTF-8 bytes.
#[must_use]
pub fn uri(address: &str) -> String {
    let mut out = String::from("geo:0,0?q=");
    for byte in address.trim().bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

/// Whether any installed application handles `geo:` URIs.
///
/// Reads every desktop entry, so it is run once, off the UI thread.
#[must_use]
pub fn handler_installed() -> bool {
    cosmic::desktop::load_applications(&[], true, None).any(|app| handles_geo(&app))
}

/// Whether one desktop entry declares the `geo:` scheme. `NoDisplay` entries
/// count — the web-map handlers are hidden from menus and still open links.
fn handles_geo(app: &cosmic::desktop::DesktopEntryData) -> bool {
    app.mime_types.iter().any(|mime| {
        mime.essence_str()
            .eq_ignore_ascii_case("x-scheme-handler/geo")
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The inverse of the encoding, as a URI parser applies it to a query
    /// value: what a maps application will search for.
    fn query(uri: &str) -> String {
        let value = uri.strip_prefix("geo:0,0?q=").expect("the geo: prefix");
        let mut bytes = Vec::new();
        let mut rest = value.as_bytes();
        while let Some((&first, tail)) = rest.split_first() {
            if first == b'%' {
                let hex = std::str::from_utf8(&tail[..2]).unwrap();
                bytes.push(u8::from_str_radix(hex, 16).unwrap());
                rest = &tail[2..];
            } else {
                bytes.push(first);
                rest = tail;
            }
        }
        String::from_utf8(bytes).expect("UTF-8")
    }

    #[test]
    fn an_address_becomes_a_geo_search() {
        assert_eq!(
            uri("1 Main St, Athens, 10431, GR"),
            "geo:0,0?q=1%20Main%20St%2C%20Athens%2C%2010431%2C%20GR"
        );
    }

    /// An address is other people's data. Nothing in it may end the query,
    /// start another parameter, or reach the opener as anything but one
    /// value — and what the maps application decodes is the address itself.
    #[test]
    fn nothing_in_an_address_escapes_the_query_value() {
        for address in [
            "Flat 2 & 3, Rue #5, Paris",
            "a=b?c/d+e;f:g@h",
            "10 Downing St\nLondon",
            "100%",
            "'quoted' \"too\" `and` $(this)",
        ] {
            let built = uri(address);
            let value = built.strip_prefix("geo:0,0?q=").unwrap();
            assert!(
                value
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-._~%".contains(&b)),
                "{address:?} left a reserved character in {built}"
            );
            assert_eq!(query(&built), address, "{built}");
        }
    }

    #[test]
    fn a_greek_address_travels_as_utf8() {
        let address = "Πανεπιστημίου 30, Αθήνα";
        let built = uri(address);
        assert!(built.is_ascii(), "{built}");
        assert_eq!(query(&built), address);
    }

    #[test]
    fn surrounding_whitespace_is_not_searched_for() {
        assert_eq!(uri("  Athens \n"), uri("Athens"));
    }

    fn entry(dir: &std::path::Path, name: &str, body: &str) -> cosmic::desktop::DesktopEntryData {
        let path = dir.join(name);
        std::fs::write(&path, body).unwrap();
        cosmic::desktop::load_desktop_file(&[], path).expect("a readable desktop entry")
    }

    /// Hidden or not, an application that declares `geo:` opens it; one that
    /// does not, does not count.
    #[test]
    fn a_geo_handler_is_recognised_by_its_mime_type() {
        let dir = tempfile::tempdir().unwrap();
        let maps = entry(
            dir.path(),
            "maps.desktop",
            "[Desktop Entry]\nType=Application\nName=Maps\nExec=maps %u\n\
MimeType=application/vnd.geo+json;x-scheme-handler/geo;x-scheme-handler/maps;\n",
        );
        let hidden = entry(
            dir.path(),
            "osm-geo-handler.desktop",
            "[Desktop Entry]\nType=Application\nName=OpenStreetMap\nExec=handler %u\n\
NoDisplay=true\nMimeType=x-scheme-handler/geo;\n",
        );
        let other = entry(
            dir.path(),
            "editor.desktop",
            "[Desktop Entry]\nType=Application\nName=Editor\nExec=editor %f\n\
MimeType=text/plain;x-scheme-handler/maps;\n",
        );
        assert!(handles_geo(&maps));
        assert!(handles_geo(&hidden));
        assert!(!handles_geo(&other));
    }
}
