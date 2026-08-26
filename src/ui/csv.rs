// SPDX-License-Identifier: GPL-3.0-only

//! CSV import, with an explicit column-mapping screen.
//!
//! # Why a mapping screen at all
//!
//! There is no CSV contact format — every exporter names its columns
//! differently, in its own language, in its own order. An importer that
//! guesses silently files ten thousand phone numbers under "nickname" and the
//! user finds out weeks later, one contact at a time. So nothing is imported
//! until the user has seen every column, what it maps to, and a sample of its
//! data, and pressed Import. Obvious headers are *pre-selected* — showing
//! "email → Email" for confirmation is help, not guessing — but the
//! pre-selection is on screen and editable, never applied behind the user's
//! back.
//!
//! # What a mapped UID buys
//!
//! If a column is mapped to UID, the import becomes repeatable the same way
//! `.vcf` import is: a row whose UID already exists **updates** that contact —
//! through the patcher, so a synced card keeps its photo — instead of piling
//! up a duplicate per import. Without one, every row is a new contact.

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;
use cosmic_pim_core::model::{Contact, Typed};
use std::path::Path;

use crate::fl;

/// What one CSV column becomes.
///
/// `Email` and `Phone` may be chosen by several columns at once — exporters
/// routinely ship "E-mail 1"/"E-mail 2" — and each mapped column appends one
/// entry.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum Target {
    #[default]
    Skip,
    GivenName,
    FamilyName,
    DisplayName,
    Nickname,
    Email,
    Phone,
    Url,
    Organisation,
    JobTitle,
    Street,
    Locality,
    Region,
    PostalCode,
    Country,
    Birthday,
    Categories,
    Note,
    Uid,
}

impl Target {
    /// Dropdown order. `Skip` first, so the zero choice is the safe one.
    pub const ALL: &'static [Target] = &[
        Target::Skip,
        Target::GivenName,
        Target::FamilyName,
        Target::DisplayName,
        Target::Nickname,
        Target::Email,
        Target::Phone,
        Target::Url,
        Target::Organisation,
        Target::JobTitle,
        Target::Street,
        Target::Locality,
        Target::Region,
        Target::PostalCode,
        Target::Country,
        Target::Birthday,
        Target::Categories,
        Target::Note,
        Target::Uid,
    ];

    fn label(self) -> String {
        match self {
            Target::Skip => fl!("csv-skip"),
            Target::GivenName => fl!("name-given"),
            Target::FamilyName => fl!("name-family"),
            Target::DisplayName => fl!("display-name"),
            Target::Nickname => fl!("nickname"),
            Target::Email => fl!("email"),
            Target::Phone => fl!("phone"),
            Target::Url => fl!("website"),
            Target::Organisation => fl!("organisation"),
            Target::JobTitle => fl!("title"),
            Target::Street => fl!("address-street"),
            Target::Locality => fl!("address-locality"),
            Target::Region => fl!("address-region"),
            Target::PostalCode => fl!("address-postcode"),
            Target::Country => fl!("address-country"),
            Target::Birthday => fl!("birthday"),
            Target::Categories => fl!("categories"),
            Target::Note => fl!("note"),
            Target::Uid => fl!("csv-uid"),
        }
    }

    /// The pre-selection for a header, matched conservatively: exact-ish
    /// names only ("email", "e-mail 1", "first name"), never fuzzy — a wrong
    /// pre-selection the user does not notice is the guessing this screen
    /// exists to prevent.
    fn suggest(header: &str) -> Target {
        let h: String = header
            .to_lowercase()
            .chars()
            .filter(|c| c.is_ascii_alphanumeric())
            .collect();
        // Numbered variants ("email1", "phone2") map like their base name.
        let base = h.trim_end_matches(|c: char| c.is_ascii_digit());
        match base {
            "email" | "emailaddress" | "mail" => Target::Email,
            "phone" | "phonenumber" | "tel" | "telephone" | "mobile" | "mobilephone" | "cell" => {
                Target::Phone
            }
            "firstname" | "givenname" => Target::GivenName,
            "lastname" | "familyname" | "surname" => Target::FamilyName,
            "name" | "fullname" | "displayname" => Target::DisplayName,
            "nickname" => Target::Nickname,
            "url" | "website" | "webpage" | "homepage" => Target::Url,
            "organisation" | "organization" | "company" | "org" => Target::Organisation,
            "title" | "jobtitle" | "role" => Target::JobTitle,
            "street" | "address" | "streetaddress" => Target::Street,
            "city" | "locality" | "town" => Target::Locality,
            "state" | "region" | "province" => Target::Region,
            "zip" | "zipcode" | "postcode" | "postalcode" => Target::PostalCode,
            "country" => Target::Country,
            "birthday" | "bday" | "dateofbirth" | "dob" => Target::Birthday,
            "categories" | "category" | "groups" | "tags" | "labels" => Target::Categories,
            "note" | "notes" | "comment" => Target::Note,
            "uid" | "id" | "uuid" => Target::Uid,
            _ => Target::Skip,
        }
    }
}

#[derive(Clone, Debug)]
pub enum Message {
    /// Column `index` remapped to `Target::ALL[choice]`.
    Mapped(usize, usize),
}

/// The parsed file plus the user's mapping.
pub struct State {
    /// The file's column headers, verbatim.
    pub headers: Vec<String>,
    /// Every data row.
    pub rows: Vec<Vec<String>>,
    /// One target per column.
    pub mapping: Vec<Target>,
    /// The first non-empty value per column, shown so the user maps data they
    /// can see rather than a header they have to trust.
    samples: Vec<String>,
    /// Dropdown labels, built once — `widget::dropdown` borrows a slice.
    target_labels: Vec<String>,
}

impl State {
    /// Parses a CSV file into a mapping screen. `Err` is a message for a toast.
    pub fn open(path: &Path) -> Result<Self, String> {
        let mut reader = csv::ReaderBuilder::new()
            .flexible(true) // ragged rows are the norm in the wild
            .from_path(path)
            .map_err(|why| why.to_string())?;

        let headers: Vec<String> = reader
            .headers()
            .map_err(|why| why.to_string())?
            .iter()
            .map(str::to_owned)
            .collect();
        if headers.is_empty() {
            return Err(fl!("csv-empty"));
        }

        let rows: Vec<Vec<String>> = reader
            .records()
            .filter_map(Result::ok)
            .map(|record| record.iter().map(str::to_owned).collect())
            .collect();
        if rows.is_empty() {
            return Err(fl!("csv-empty"));
        }

        let samples = (0..headers.len())
            .map(|column| {
                rows.iter()
                    .filter_map(|row| row.get(column))
                    .find(|v| !v.trim().is_empty())
                    .cloned()
                    .unwrap_or_default()
            })
            .collect();

        let mapping = headers.iter().map(|h| Target::suggest(h)).collect();

        Ok(Self {
            headers,
            rows,
            mapping,
            samples,
            target_labels: Target::ALL.iter().map(|t| t.label()).collect(),
        })
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Mapped(index, choice) => {
                if let (Some(slot), Some(target)) =
                    (self.mapping.get_mut(index), Target::ALL.get(choice))
                {
                    *slot = *target;
                }
            }
        }
    }

    /// Whether anything at all would be imported.
    #[must_use]
    pub fn is_importable(&self) -> bool {
        self.mapping.iter().any(|t| *t != Target::Skip)
    }

    /// Applies the mapping: one contact per row that yields a non-empty label.
    ///
    /// Rows that map to nothing displayable are dropped and counted — an
    /// unnamed contact is a row nobody can ever find again, so importing it
    /// would be worse than saying it was skipped.
    #[must_use]
    pub fn contacts(&self, book_id: &str) -> (Vec<Contact>, usize) {
        let mut out = Vec::new();
        let mut skipped = 0usize;

        for row in &self.rows {
            let mut contact = Contact::draft(book_id);
            for (column, target) in self.mapping.iter().enumerate() {
                let Some(value) = row.get(column).map(|v| v.trim()) else {
                    continue;
                };
                if value.is_empty() {
                    continue;
                }
                apply(&mut contact, *target, value);
            }
            if contact.label().trim().is_empty() {
                skipped += 1;
                continue;
            }
            out.push(contact);
        }
        (out, skipped)
    }
}

/// Applies one cell to the draft.
fn apply(contact: &mut Contact, target: Target, value: &str) {
    // At most one address per row; the components share it.
    fn address(contact: &mut Contact) -> &mut cosmic_pim_core::model::Address {
        if contact.addresses.is_empty() {
            contact.addresses.push(Default::default());
        }
        contact.addresses.last_mut().expect("just pushed")
    }

    match target {
        Target::Skip => {}
        Target::GivenName => contact.name.given = value.to_owned(),
        Target::FamilyName => contact.name.family = value.to_owned(),
        Target::DisplayName => contact.display_name = value.to_owned(),
        Target::Nickname => contact.nicknames.push(value.to_owned()),
        Target::Email => contact.emails.push(Typed::new(value)),
        Target::Phone => contact.phones.push(Typed::new(value)),
        Target::Url => contact.urls.push(Typed::new(value)),
        Target::Organisation => contact.organisation = Some(value.to_owned()),
        Target::JobTitle => contact.title = Some(value.to_owned()),
        Target::Street => address(contact).street = value.to_owned(),
        Target::Locality => address(contact).locality = value.to_owned(),
        Target::Region => address(contact).region = value.to_owned(),
        Target::PostalCode => address(contact).postal_code = value.to_owned(),
        Target::Country => address(contact).country = value.to_owned(),
        // ISO only, deliberately: "03/04/05" is three different days depending
        // on which side of an ocean the exporter lived, and a birthday filed
        // eleven months off is worse than one left blank.
        Target::Birthday => {
            contact.birthday = chrono::NaiveDate::parse_from_str(value, "%Y-%m-%d").ok();
        }
        Target::Categories => contact.categories.extend(
            value
                .split([',', ';'])
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(ToOwned::to_owned),
        ),
        Target::Note => contact.note = Some(value.to_owned()),
        Target::Uid => contact.uid = value.to_owned(),
    }
}

/// The mapping screen.
pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut section =
        widget::settings::section().title(fl!("csv-columns", count = state.rows.len().to_string()));

    for (index, header) in state.headers.iter().enumerate() {
        let selected = Target::ALL
            .iter()
            .position(|t| Some(t) == state.mapping.get(index));

        let mut item = widget::settings::item::builder(header.clone());
        if let Some(sample) = state.samples.get(index).filter(|s| !s.is_empty()) {
            item = item.description(sample.clone());
        }
        section = section.add(item.control(widget::dropdown(
            &state.target_labels,
            selected,
            move |choice| Message::Mapped(index, choice),
        )));
    }

    widget::scrollable(
        widget::column::with_capacity(1)
            .push(section)
            .padding(spacing.space_s),
    )
    .height(Length::Fill)
    .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(headers: &[&str], rows: &[&[&str]]) -> State {
        State {
            headers: headers.iter().map(ToString::to_string).collect(),
            rows: rows
                .iter()
                .map(|r| r.iter().map(ToString::to_string).collect())
                .collect(),
            mapping: headers.iter().map(|h| Target::suggest(h)).collect(),
            samples: Vec::new(),
            target_labels: Vec::new(),
        }
    }

    #[test]
    fn obvious_headers_are_pre_selected_and_odd_ones_are_not() {
        let s = state(
            &["First Name", "E-mail 1", "Mobile Phone", "Frobnicator"],
            &[],
        );
        assert_eq!(
            s.mapping,
            vec![
                Target::GivenName,
                Target::Email,
                Target::Phone,
                Target::Skip
            ]
        );
    }

    #[test]
    fn rows_become_contacts_and_multi_mapped_columns_append() {
        let s = state(
            &["First Name", "Last Name", "E-mail 1", "E-mail 2"],
            &[
                &["Ada", "Lovelace", "ada@work.example", "ada@home.example"],
                &["Alan", "Turing", "alan@example.com", ""],
            ],
        );
        let (contacts, skipped) = s.contacts("book");
        assert_eq!(skipped, 0);
        assert_eq!(contacts.len(), 2);
        assert_eq!(contacts[0].label(), "Ada Lovelace");
        assert_eq!(
            contacts[0].emails.len(),
            2,
            "the second email column was lost"
        );
        assert_eq!(contacts[1].emails.len(), 1);
    }

    /// A row with no name, organisation, or email would import as an unnamed
    /// contact nobody can find; it is skipped and counted instead.
    #[test]
    fn unnameable_rows_are_skipped_and_counted() {
        let s = state(
            &["First Name", "Phone"],
            &[&["", "+30 210 0000000"], &["Ada", "+30 210 1111111"]],
        );
        let (contacts, skipped) = s.contacts("book");
        assert_eq!(contacts.len(), 1);
        assert_eq!(skipped, 1);
    }

    #[test]
    fn address_components_share_one_address() {
        let s = state(
            &["Name", "Street", "City", "ZIP"],
            &[&["Ada", "1 Main St", "Athens", "10431"]],
        );
        let (contacts, _) = s.contacts("book");
        assert_eq!(
            contacts[0].addresses.len(),
            1,
            "each component grew its own address"
        );
        assert_eq!(
            contacts[0].addresses[0].one_line(),
            "1 Main St, Athens, 10431"
        );
    }

    #[test]
    fn birthdays_parse_iso_only() {
        let s = state(
            &["Name", "Birthday"],
            &[&["Ada", "1815-12-10"], &["Alan", "23/06/1912"]],
        );
        let (contacts, _) = s.contacts("book");
        assert_eq!(
            contacts[0].birthday,
            chrono::NaiveDate::from_ymd_opt(1815, 12, 10)
        );
        assert!(
            contacts[1].birthday.is_none(),
            "an ambiguous date format was guessed at"
        );
    }

    #[test]
    fn a_mapped_uid_lands_on_the_contact() {
        let s = state(&["Name", "UID"], &[&["Ada", "ada@import"]]);
        let (contacts, _) = s.contacts("book");
        assert_eq!(contacts[0].uid, "ada@import");
    }

    #[test]
    fn nothing_mapped_means_nothing_importable() {
        let mut s = state(&["Frobnicator"], &[&["x"]]);
        assert!(!s.is_importable());
        s.update(Message::Mapped(0, 3)); // DisplayName
        assert!(s.is_importable());
    }

    /// The real parser handles quoted commas — the reason the csv crate is a
    /// dependency at all.
    #[test]
    fn quoted_fields_survive_parsing() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("import.csv");
        std::fs::write(
            &path,
            "Name,Organisation\n\"Lovelace, Ada\",\"Engines, Analytical \"\"Ltd\"\"\"\n",
        )
        .unwrap();

        let state = State::open(&path).unwrap();
        let (contacts, _) = state.contacts("book");
        assert_eq!(contacts[0].display_name, "Lovelace, Ada");
        assert_eq!(
            contacts[0].organisation.as_deref(),
            Some("Engines, Analytical \"Ltd\"")
        );
    }
}
