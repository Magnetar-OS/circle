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
    pub organisation: Option<String>,
    pub nicknames: Vec<String>,
    pub fields: Vec<Field<'a>>,
    pub categories: Vec<String>,
    /// Every card underneath, with its book's display name — what the
    /// "Linked cards" section lists and unlinks from.
    pub cards: &'a [(&'a Contact, &'a str)],
}

impl Composed<'_> {
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
    if let Some((contact, book)) = cards.iter().find(|(c, _)| c.birthday.is_some()) {
        let birthday = contact.birthday.expect("just filtered for it");
        fields.push(Field {
            label: crate::fl!("birthday"),
            value: birthday.format("%-d %B %Y").to_string(),
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

    let organisation = cards.iter().find_map(|(contact, _)| {
        let org = contact.organisation.as_ref()?;
        Some(match &contact.title {
            Some(title) if !title.trim().is_empty() => format!("{title}, {org}"),
            _ => org.clone(),
        })
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
