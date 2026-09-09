// SPDX-License-Identifier: GPL-3.0-only

//! Who a contact is related to, read off their card.
//!
//! vCard has two ways of saying this and Circle reads both, because which one
//! a card uses is decided by whatever wrote it, not by the user:
//!
//! - **`RELATED`** (RFC 6350, vCard 4.0). The value is a URI —
//!   `urn:uuid:…`, `mailto:…` — or free text, and the relationship is a
//!   `TYPE=` parameter.
//! - **`X-ABRELATEDNAMES`**, what Apple Contacts writes and therefore what
//!   most real 3.0 cards carry. The value is a plain name, and the label is a
//!   *sibling line in the same group*: `item1.X-ABRELATEDNAMES:Jane Doe`
//!   alongside `item1.X-ABLabel:_$!<Spouse>!$_`.
//!
//! # Read-only, and why
//!
//! Circle displays relationships and navigates them; it does not edit them.
//! Writing either property back means a byte-preserving patcher for it in the
//! substrate — the same machinery `set_group_members` is — and the substrate
//! does not model contact `RELATED` at all today. Reading needs none of that,
//! and a card that arrives from another client with relationships on it
//! becomes navigable here rather than being invisible.
//!
//! # Resolving a target
//!
//! A relationship is useful when you can click it. `urn:uuid:` and bare UIDs
//! resolve against contact UIDs, `mailto:` against addresses, and a plain
//! name against display names. What does not resolve is shown as text —
//! which is the common case for a 3.0 card naming somebody by name, and 03 §7
//! asks for exactly that fallback rather than hiding it.

use cosmic_pim_core::model::Contact;

use crate::app::ContactKey;

/// One relationship, as the card states it.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Relation {
    /// The relationship itself — "spouse", "Sister", "manager".
    pub label: String,
    /// The person, as the card names them: a resolved contact's label, or the
    /// card's own text when nothing resolves.
    pub name: String,
    /// The contact this points at, when one could be found.
    pub target: Option<ContactKey>,
}

/// Every relationship on a card, in the order the card lists them.
///
/// `others` is what targets are resolved against — normally every contact in
/// a visible book.
#[must_use]
pub fn relations(raw: &str, others: &[Contact]) -> Vec<Relation> {
    use cosmic_pim_core::patch::logical_lines;

    let lines = logical_lines(raw);

    // Apple's labels live on a sibling line in the same group, so collect
    // them before walking the values that refer to them.
    let mut labels: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    for line in &lines {
        if line.name() == "X-ABLABEL"
            && let Some(group) = line.group()
        {
            labels.insert(group.to_owned(), apple_label(line.value()));
        }
    }

    let mut out = Vec::new();
    for line in &lines {
        let (value, label) = match line.name().as_str() {
            "RELATED" => (line.value().to_owned(), type_param(line.params())),
            "X-ABRELATEDNAMES" => {
                let label = line
                    .group()
                    .and_then(|group| labels.get(group).cloned())
                    .unwrap_or_default();
                (line.value().to_owned(), label)
            }
            _ => continue,
        };

        let value = unescape(&value);
        if value.trim().is_empty() {
            continue;
        }
        let target = resolve(&value, others);
        let name = target
            .as_ref()
            .and_then(|key| others.iter().find(|c| key.matches(c)))
            .map_or_else(|| display_value(&value), Contact::label);

        out.push(Relation {
            label: if label.is_empty() {
                crate::fl!("related")
            } else {
                label
            },
            name,
            target,
        });
    }
    out
}

/// The contact a relationship's value points at, if any.
fn resolve(value: &str, others: &[Contact]) -> Option<ContactKey> {
    // A UID, in either of the two spellings MEMBER uses — the substrate
    // already owns that parsing, and RELATED spells them the same way.
    if let Some(uid) = cosmic_pim_core::vcard::member_uid(value)
        && let Some(contact) = others.iter().find(|c| c.uid == uid)
    {
        return Some(ContactKey::of(contact));
    }

    if let Some(address) = value.strip_prefix("mailto:") {
        let address = address.trim().to_lowercase();
        if let Some(contact) = others.iter().find(|c| {
            c.emails
                .iter()
                .any(|e| e.value.trim().to_lowercase() == address)
        }) {
            return Some(ContactKey::of(contact));
        }
    }

    // A plain name — what an Apple card carries, and what a 4.0 card with
    // `VALUE=text` carries. Matched case-insensitively against display names;
    // an ambiguous name resolves to nobody rather than to a guess.
    let name = display_value(value).trim().to_lowercase();
    if name.is_empty() {
        return None;
    }
    let mut matches = others
        .iter()
        .filter(|c| c.label().trim().to_lowercase() == name);
    let first = matches.next()?;
    if matches.next().is_some() {
        return None; // Two people share the name; picking one would be a guess.
    }
    Some(ContactKey::of(first))
}

/// A value with its URI scheme stripped, for showing to a reader.
fn display_value(value: &str) -> String {
    value
        .strip_prefix("mailto:")
        .unwrap_or(value)
        .trim()
        .to_owned()
}

/// The `TYPE=` parameter, lowercased to one word.
///
/// `RELATED;TYPE=spouse:` and `RELATED;VALUE=text;TYPE=friend:` both answer.
fn type_param(params: &str) -> String {
    for part in params.split(';') {
        let Some((name, value)) = part.split_once('=') else {
            continue;
        };
        if name.trim().eq_ignore_ascii_case("type") {
            // A multi-valued TYPE takes its first entry; the rest are
            // qualifiers a one-line label has no room for.
            return value
                .trim()
                .trim_matches('"')
                .split(',')
                .next()
                .unwrap_or_default()
                .to_lowercase();
        }
    }
    String::new()
}

/// Apple wraps its built-in labels in `_$!<…>!$_`; a user's own label is
/// stored bare.
fn apple_label(value: &str) -> String {
    let value = unescape(value);
    value
        .strip_prefix("_$!<")
        .and_then(|rest| rest.strip_suffix(">!$_"))
        .unwrap_or(&value)
        .to_owned()
}

/// vCard escaping, undone: `\,` `\;` `\\` and `\n`.
fn unescape(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    let mut chars = value.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n' | 'N') => out.push('\n'),
            Some(other) => out.push(other),
            None => out.push('\\'),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(book: &str, uid: &str, name: &str) -> Contact {
        let mut c = Contact::draft(book);
        c.uid = uid.into();
        c.display_name = name.into();
        c
    }

    fn with_email(mut c: Contact, address: &str) -> Contact {
        c.emails.push(cosmic_pim_core::model::Typed {
            value: address.into(),
            types: Vec::new(),
            pref: None,
            group: None,
            params: Vec::new(),
        });
        c
    }

    fn card(body: &str) -> String {
        format!("BEGIN:VCARD\r\nVERSION:4.0\r\nUID:me\r\nFN:Me\r\n{body}END:VCARD\r\n")
    }

    #[test]
    fn a_related_uid_resolves_to_that_contact() {
        let others = [contact("personal", "ada", "Ada Lovelace")];
        let found = relations(&card("RELATED;TYPE=friend:urn:uuid:ada\r\n"), &others);

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].label, "friend");
        assert_eq!(found[0].name, "Ada Lovelace");
        assert_eq!(found[0].target, Some(ContactKey::of(&others[0])));
    }

    #[test]
    fn a_related_mailto_resolves_by_address() {
        let others = [with_email(
            contact("personal", "ada", "Ada Lovelace"),
            "ADA@Example.org",
        )];
        let found = relations(
            &card("RELATED;TYPE=colleague:mailto:ada@example.org\r\n"),
            &others,
        );

        assert_eq!(found[0].target, Some(ContactKey::of(&others[0])));
        assert_eq!(found[0].name, "Ada Lovelace");
    }

    /// The 3.0 case 03 §7 asks for: a name that points at nobody is still
    /// shown, as text.
    #[test]
    fn an_unresolvable_relation_is_kept_as_text() {
        let found = relations(
            &card("RELATED;VALUE=text;TYPE=sibling:Someone Not In The Book\r\n"),
            &[],
        );

        assert_eq!(found.len(), 1);
        assert_eq!(found[0].name, "Someone Not In The Book");
        assert!(found[0].target.is_none());
    }

    /// What Apple Contacts actually writes, which is most real cards.
    #[test]
    fn apple_related_names_take_their_label_from_the_group() {
        let others = [contact("personal", "jane", "Jane Doe")];
        let found = relations(
            &card(
                "item1.X-ABRELATEDNAMES:Jane Doe\r\n\
                 item1.X-ABLabel:_$!<Spouse>!$_\r\n",
            ),
            &others,
        );

        assert_eq!(found.len(), 1);
        assert_eq!(
            found[0].label, "Spouse",
            "the Apple wrapper was not stripped"
        );
        assert_eq!(found[0].target, Some(ContactKey::of(&others[0])));
    }

    #[test]
    fn an_apple_custom_label_is_used_verbatim() {
        let found = relations(
            &card(
                "item2.X-ABRELATEDNAMES:Kostas\r\n\
                 item2.X-ABLabel:Best man\r\n",
            ),
            &[],
        );
        assert_eq!(found[0].label, "Best man");
    }

    /// Two people with one name is ambiguous; resolving to either would be a
    /// guess, and a wrong guess opens the wrong person.
    #[test]
    fn an_ambiguous_name_resolves_to_nobody() {
        let others = [
            contact("personal", "a", "Ada Lovelace"),
            contact("work", "b", "Ada Lovelace"),
        ];
        let found = relations(&card("RELATED;VALUE=text:Ada Lovelace\r\n"), &others);

        assert_eq!(found.len(), 1);
        assert!(
            found[0].target.is_none(),
            "picked one of two same-named people"
        );
        assert_eq!(found[0].name, "Ada Lovelace");
    }

    #[test]
    fn a_relation_with_no_type_still_shows() {
        let found = relations(&card("RELATED:Someone\r\n"), &[]);
        assert_eq!(found.len(), 1);
        assert!(
            !found[0].label.is_empty(),
            "a relation with no TYPE lost its label"
        );
    }

    #[test]
    fn a_card_with_no_relationships_has_none() {
        assert!(relations(&card("EMAIL:me@example.org\r\n"), &[]).is_empty());
    }

    #[test]
    fn escaped_separators_survive() {
        let found = relations(&card("RELATED;VALUE=text:Doe\\, Jane\r\n"), &[]);
        assert_eq!(found[0].name, "Doe, Jane");
    }

    #[test]
    fn a_multi_valued_type_takes_its_first_entry() {
        let found = relations(&card("RELATED;TYPE=\"friend,colleague\":Someone\r\n"), &[]);
        assert_eq!(found[0].label, "friend");
    }
}
