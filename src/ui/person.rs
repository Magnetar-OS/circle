// SPDX-License-Identifier: GPL-3.0-only

//! Composing one person out of several cards.
//!
//! A linked person (03 §5) is shown as one entry whose values come from every
//! card underneath it. The cards themselves are never touched: this is a read
//! model built fresh for the detail pane, and every edit still lands on
//! exactly one card — the one a given value actually came from.
//!
//! Two rules do all the work:
//!
//! - **First card wins a scalar.** Organisation, job title, birthday, note:
//!   the precedence head's value if it has one, else the next card's. The
//!   head is the first card in the link record, which is the first card the
//!   user picked when linking.
//! - **Lists union, and duplicates collapse.** Two cards for one person
//!   usually share an address or a number — that is how the duplicate finder
//!   spotted them. Showing it twice would make linking look like it made
//!   things worse, so a repeated value appears once, attributed to the first
//!   card carrying it.
//!
//! Composing a single card produces exactly that card, with no source
//! attribution anywhere — the unlinked case pays nothing for this existing.

use cosmic_pim_core::model::Contact;

/// A birthday with no year, as a day and a month.
///
/// Formatted through a date in a leap year so the month name is localised by
/// the same machinery as a full birthday, and so 29 February is expressible.
/// The year is then not shown, because the card does not claim one.
fn ageless_birthday(month: u32, day: u32) -> String {
    chrono::NaiveDate::from_ymd_opt(2024, month, day).map_or_else(
        || format!("{day:02}-{month:02}"),
        |date| date.format("%-d %B").to_string(),
    )
}

/// One labelled value in the detail pane, and where it came from.
#[derive(Clone, Debug)]
pub struct Field<'a> {
    pub label: String,
    pub value: String,
    /// A URI the value can be acted on with — `mailto:`, `tel:`, a website.
    pub action: Option<String>,
    /// The icon for that action; empty when there is none.
    pub icon: &'static str,
    /// The bare number, when this field is a phone — what an SMS is
    /// addressed to. Set only for phones, so the renderer can offer texting
    /// without re-deriving what kind of field it is looking at.
    pub number: Option<String>,
    /// The book this value came from. `None` for an unlinked contact, where
    /// there is only one source and saying so would be noise.
    pub source: Option<&'a str>,
}

/// A person as the detail pane needs them.
#[derive(Debug)]
pub struct Composed<'a> {
    /// The card edits and deletes default to.
    pub head: &'a Contact,
    pub label: String,
    /// The company, as the card states it — *not* a display string. A
    /// heading that folds in the job title and the department levels is a
    /// view concern; writing one of those into an `ORG:` line files the
    /// contact under a company literally named "Mathematician, Acme ‣ R&D",
    /// which is what happened before these were kept apart.
    pub organisation: Option<String>,
    /// `ORG`'s further components — department, team.
    pub organisation_units: Vec<String>,
    pub title: Option<String>,
    pub nicknames: Vec<String>,
    pub fields: Vec<Field<'a>>,
    pub categories: Vec<String>,
    /// Every card underneath, with its book's display name — what the
    /// "Linked cards" section lists and unlinks from.
    pub cards: &'a [(&'a Contact, &'a str)],
}

impl Composed<'_> {
    /// The one line under the name: job title, company, and the department
    /// levels beneath it.
    ///
    /// A view concern, built here so both panes agree and so nothing is
    /// tempted to write it into a data field.
    #[must_use]
    pub fn heading(&self) -> Option<String> {
        let mut full = self.organisation.clone()?;
        for unit in &self.organisation_units {
            full.push_str(" \u{2023} ");
            full.push_str(unit);
        }
        Some(match &self.title {
            Some(title) => format!("{title}, {full}"),
            None => full,
        })
    }

    /// Whether this person is more than one card.
    #[must_use]
    pub fn is_linked(&self) -> bool {
        self.cards.len() > 1
    }
}

/// Builds the read model. `cards` is in precedence order, head first, each
/// paired with its book's display name.
#[must_use]
pub fn compose<'a>(cards: &'a [(&'a Contact, &'a str)]) -> Option<Composed<'a>> {
    let (head, _) = *cards.first()?;
    // Attribution is meaningless with one source, and a book name beside
    // every row of an ordinary contact is clutter.
    let linked = cards.len() > 1;
    let attribute = |book: &'a str| linked.then_some(book);

    let mut fields: Vec<Field<'a>> = Vec::new();
    let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();

    for (contact, book) in cards {
        for email in &contact.emails {
            if seen.insert(format!("email:{}", email.value.trim().to_lowercase())) {
                fields.push(Field {
                    label: email.label().unwrap_or("email").to_owned(),
                    value: email.value.clone(),
                    action: Some(format!("mailto:{}", email.value)),
                    icon: "mail-send-symbolic",
                    number: None,
                    source: attribute(book),
                });
            }
        }
    }
    for (contact, book) in cards {
        for phone in &contact.phones {
            // Keyed the way the duplicate finder keys them, so two spellings
            // of one number collapse into the row the user expects.
            let key = crate::dedupe::phone_key(&phone.value)
                .unwrap_or_else(|| phone.value.trim().to_lowercase());
            if seen.insert(format!("phone:{key}")) {
                fields.push(Field {
                    label: phone.label().unwrap_or("phone").to_owned(),
                    value: phone.value.clone(),
                    // `tel:` is handed to the desktop's handler. Without one
                    // nothing happens, which is why the value stays copyable.
                    action: Some(format!("tel:{}", phone.value.replace(' ', ""))),
                    icon: "call-start-symbolic",
                    number: Some(phone.value.clone()),
                    source: attribute(book),
                });
            }
        }
    }
    for (contact, book) in cards {
        for address in &contact.addresses {
            let line = address.one_line();
            if seen.insert(format!("address:{}", line.to_lowercase())) {
                fields.push(Field {
                    label: address
                        .types
                        .first()
                        .cloned()
                        .unwrap_or_else(|| crate::fl!("address")),
                    value: line,
                    action: None,
                    icon: "",
                    number: None,
                    source: attribute(book),
                });
            }
        }
    }
    for (contact, book) in cards {
        for url in &contact.urls {
            if seen.insert(format!("url:{}", url.value.trim().to_lowercase())) {
                fields.push(Field {
                    label: url.label().unwrap_or("website").to_owned(),
                    value: url.value.clone(),
                    action: Some(url.value.clone()),
                    icon: "web-browser-symbolic",
                    number: None,
                    source: attribute(book),
                });
            }
        }
    }

    // Scalars: the first card that has one.
    // A card carries either a full date or a year-less one — `BDAY:--0415` is
    // legal vCard and common from people who would rather not state an age.
    // Showing only the first left the second invisible, which reads as though
    // the card had no birthday on it at all.
    if let Some((contact, book)) = cards
        .iter()
        .find(|(c, _)| c.birthday.is_some() || c.birthday_month_day.is_some())
    {
        let value = match (contact.birthday, contact.birthday_month_day) {
            (Some(date), _) => date.format("%-d %B %Y").to_string(),
            (None, Some((month, day))) => ageless_birthday(month, day),
            (None, None) => unreachable!("just filtered for one of them"),
        };
        fields.push(Field {
            label: crate::fl!("birthday"),
            value,
            action: None,
            icon: "",
            number: None,
            source: attribute(book),
        });
    }
    if let Some((contact, book)) = cards
        .iter()
        .find(|(c, _)| c.note.as_ref().is_some_and(|n| !n.trim().is_empty()))
    {
        fields.push(Field {
            label: crate::fl!("note"),
            value: contact.note.clone().unwrap_or_default(),
            action: None,
            icon: "",
            number: None,
            source: attribute(book),
        });
    }

    // The first card that states a company brings its whole ORG with it —
    // taking the company from one card and its departments from another would
    // describe a workplace that does not exist.
    let employer = cards
        .iter()
        .find(|(contact, _)| contact.organisation.is_some());
    let organisation = employer.and_then(|(contact, _)| contact.organisation.clone());
    let organisation_units = employer.map_or_else(Vec::new, |(contact, _)| {
        contact
            .organisation_units
            .iter()
            .filter(|unit| !unit.trim().is_empty())
            .cloned()
            .collect()
    });
    let title = cards.iter().find_map(|(contact, _)| {
        contact
            .title
            .as_ref()
            .filter(|title| !title.trim().is_empty())
            .cloned()
    });

    let mut nicknames: Vec<String> = Vec::new();
    let mut categories: Vec<String> = Vec::new();
    for (contact, _) in cards {
        for nickname in &contact.nicknames {
            if !nicknames.contains(nickname) {
                nicknames.push(nickname.clone());
            }
        }
        for category in &contact.categories {
            if !categories.contains(category) {
                categories.push(category.clone());
            }
        }
    }

    Some(Composed {
        head,
        label: head.label(),
        organisation,
        organisation_units,
        title,
        nicknames,
        fields,
        categories,
        cards,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_pim_core::model::Typed;

    fn typed(value: &str) -> Typed {
        Typed {
            value: value.into(),
            types: Vec::new(),
            pref: None,
            group: None,
            params: Vec::new(),
        }
    }

    fn contact(book: &str, uid: &str, name: &str) -> Contact {
        let mut c = Contact::draft(book);
        c.uid = uid.into();
        c.display_name = name.into();
        c
    }

    /// The unlinked case must be exactly the card, with nothing added and
    /// nothing attributed.
    #[test]
    fn one_card_composes_to_itself_with_no_attribution() {
        let mut card = contact("personal", "a", "Ada Lovelace");
        card.emails.push(typed("ada@example.org"));
        card.organisation = Some("Analytical Engine".into());

        let cards = [(&card, "Personal")];
        let composed = compose(&cards).unwrap();

        assert!(!composed.is_linked());
        assert_eq!(composed.fields.len(), 1);
        assert_eq!(composed.fields[0].value, "ada@example.org");
        assert!(
            composed.fields[0].source.is_none(),
            "attributed a value on a contact with only one source"
        );
        assert_eq!(composed.organisation.as_deref(), Some("Analytical Engine"));
    }

    #[test]
    fn two_cards_union_their_values_and_say_which_book_each_came_from() {
        let mut work = contact("work", "a", "Ada Lovelace");
        work.emails.push(typed("ada@work.example"));
        let mut home = contact("personal", "b", "Ada");
        home.emails.push(typed("ada@home.example"));

        let cards = [(&work, "Work"), (&home, "Personal")];
        let composed = compose(&cards).unwrap();

        assert!(composed.is_linked());
        assert_eq!(composed.fields.len(), 2);
        assert_eq!(composed.fields[0].source, Some("Work"));
        assert_eq!(composed.fields[1].source, Some("Personal"));
    }

    /// The shared value is why the pair was found in the first place; showing
    /// it twice would make linking look like it made things worse.
    #[test]
    fn a_shared_address_appears_once_attributed_to_the_head() {
        let mut work = contact("work", "a", "Ada");
        work.emails.push(typed("ada@example.org"));
        let mut home = contact("personal", "b", "Ada");
        home.emails.push(typed("ADA@Example.org"));

        let cards = [(&work, "Work"), (&home, "Personal")];
        let composed = compose(&cards).unwrap();

        assert_eq!(composed.fields.len(), 1);
        assert_eq!(composed.fields[0].source, Some("Work"));
    }

    #[test]
    fn one_number_spelled_two_ways_collapses() {
        let mut work = contact("work", "a", "Ada");
        work.phones.push(typed("+1 (555) 123-4567"));
        let mut home = contact("personal", "b", "Ada");
        home.phones.push(typed("555-123-4567"));

        let cards = [(&work, "Work"), (&home, "Personal")];
        assert_eq!(compose(&cards).unwrap().fields.len(), 1);
    }

    /// A department is data the card carries; preserving it and not showing
    /// it reads as though the card said less than it does.
    #[test]
    fn the_organisations_department_levels_are_shown() {
        let mut card = contact("work", "a", "Ada");
        card.organisation = Some("Analytical Engine Co".into());
        card.organisation_units = vec!["Research".into(), "Difference Engines".into()];

        let cards = [(&card, "Work")];
        let composed = compose(&cards).unwrap();

        // The heading is the display string; the fields beside it are the
        // data. Keeping them apart is what stops a heading being written into
        // an ORG line, which is what the QR share was doing.
        let shown = composed.heading().expect("a heading");
        assert!(shown.contains("Analytical Engine Co"), "{shown}");
        assert!(shown.contains("Research"), "{shown}");
        assert!(shown.contains("Difference Engines"), "{shown}");

        assert_eq!(
            composed.organisation.as_deref(),
            Some("Analytical Engine Co")
        );
        assert_eq!(
            composed.organisation_units,
            ["Research", "Difference Engines"]
        );
    }

    /// A card that states a day and a month but no year still has a birthday
    /// on it, and showing nothing said otherwise.
    #[test]
    fn a_birthday_with_no_year_is_shown_without_inventing_one() {
        let mut card = contact("personal", "a", "Ada");
        card.birthday_month_day = Some((4, 15));

        let cards = [(&card, "Personal")];
        let composed = compose(&cards).unwrap();
        let birthday = composed
            .fields
            .iter()
            .find(|f| f.value.contains("April"))
            .expect("the birthday is shown");

        assert!(birthday.value.contains("15"), "{}", birthday.value);
        assert!(
            !birthday.value.contains("2024"),
            "a year was invented for a card that gives none: {}",
            birthday.value
        );
    }

    /// 29 February has no year-less representation in a non-leap year; the
    /// formatting must not silently drop it.
    #[test]
    fn a_leap_day_birthday_is_expressible() {
        let mut card = contact("personal", "a", "Ada");
        card.birthday_month_day = Some((2, 29));

        let cards = [(&card, "Personal")];
        let composed = compose(&cards).unwrap();
        assert!(
            composed.fields.iter().any(|f| f.value.contains("29")),
            "a leap-day birthday was lost in formatting"
        );
    }

    #[test]
    fn a_scalar_missing_from_the_head_is_taken_from_the_next_card() {
        let work = contact("work", "a", "Ada");
        let mut home = contact("personal", "b", "Ada");
        home.organisation = Some("Analytical Engine".into());

        let cards = [(&work, "Work"), (&home, "Personal")];
        let composed = compose(&cards).unwrap();
        assert_eq!(composed.organisation.as_deref(), Some("Analytical Engine"));
    }

    #[test]
    fn the_head_wins_a_scalar_both_cards_have() {
        let mut work = contact("work", "a", "Ada");
        work.organisation = Some("Work Ltd".into());
        let mut home = contact("personal", "b", "Ada");
        home.organisation = Some("Home Ltd".into());

        let cards = [(&work, "Work"), (&home, "Personal")];
        assert_eq!(
            compose(&cards).unwrap().organisation.as_deref(),
            Some("Work Ltd")
        );
    }

    #[test]
    fn categories_and_nicknames_are_unioned_without_repeats() {
        let mut work = contact("work", "a", "Ada");
        work.categories = vec!["Colleagues".into(), "Maths".into()];
        work.nicknames = vec!["Ada".into()];
        let mut home = contact("personal", "b", "Ada");
        home.categories = vec!["Maths".into(), "Friends".into()];
        home.nicknames = vec!["Ada".into(), "AL".into()];

        let cards = [(&work, "Work"), (&home, "Personal")];
        let composed = compose(&cards).unwrap();
        assert_eq!(composed.categories, ["Colleagues", "Maths", "Friends"]);
        assert_eq!(composed.nicknames, ["Ada", "AL"]);
    }

    #[test]
    fn the_head_is_the_first_card() {
        let work = contact("work", "a", "Ada Work");
        let home = contact("personal", "b", "Ada Home");
        let cards = [(&work, "Work"), (&home, "Personal")];
        let composed = compose(&cards).unwrap();
        assert_eq!(composed.head.uid, "a");
        assert_eq!(composed.label, "Ada Work");
    }
}
