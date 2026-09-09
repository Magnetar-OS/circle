// SPDX-License-Identifier: GPL-3.0-only

//! Finding cards that are probably the same person.
//!
//! Suggestions only. Nothing here writes anything, nothing merges, and the
//! review screen's default action is [`link`](crate::links::LinkStore::link),
//! which leaves both cards intact — 03 §6 is explicit that automatic merging
//! is never acceptable, because a merge is the one operation in an address
//! book that cannot be undone from the files that remain.
//!
//! Three signals, in descending confidence:
//!
//! - **Email**, matched exactly after case folding. Two cards claiming one
//!   mailbox are the same person often enough to lead with.
//! - **Phone**, matched on significant digits (below). Nearly as strong.
//! - **Name**, transliterated and compared fuzzily. Weak on its own — two
//!   people genuinely share a name — so it is labelled as a guess in the UI.
//!
//! # Why not the `phonenumber` crate
//!
//! Proper E.164 normalisation needs a default region to parse a national
//! number like `(555) 123-4567`, and Circle has no honest source for one: the
//! locale is a language preference, not a dialling plan, and guessing wrong
//! turns a correct pair into a missed one. Comparing significant trailing
//! digits gets `+1 (555) 123-4567` and `555-123-4567` together without
//! inventing a country, which is the whole job here.

use std::collections::HashMap;

use cosmic_pim_core::model::Contact;

use crate::links::{CardRef, LinkStore};

/// Digits that have to agree for two numbers to be considered the same.
/// Seven is a subscriber number in most plans — shorter starts matching
/// extensions and short codes against each other.
const PHONE_DIGITS: usize = 7;

/// Jaro-Winkler score above which two transliterated names are worth showing.
/// A suggestion threshold, not a decision boundary: everything above it is
/// still shown to the user as a guess and linked only on their say-so.
const NAME_SIMILARITY: f64 = 0.92;

/// Why a pair was proposed — shown verbatim in the review screen, so the user
/// judges the evidence rather than the app's confidence.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Reason {
    /// Both cards carry this address.
    Email(String),
    /// Both cards carry a number with the same significant digits; the value
    /// is the way the first card spells it.
    Phone(String),
    /// The names transliterate to something similar. A guess.
    Name,
}

impl Reason {
    /// Whether this is evidence rather than a hunch. Strong candidates sort
    /// first and are the ones worth a default action.
    #[must_use]
    pub fn is_strong(&self) -> bool {
        matches!(self, Self::Email(_) | Self::Phone(_))
    }
}

/// One proposed pair.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub a: CardRef,
    pub b: CardRef,
    pub reason: Reason,
}

/// Every pair worth reviewing, strongest first.
///
/// Pairs already linked into one person, and pairs the user has dismissed,
/// are left out — the screen must never re-ask a question it has had answered.
#[must_use]
pub fn candidates(contacts: &[Contact], links: &LinkStore) -> Vec<Candidate> {
    let refs: Vec<CardRef> = contacts
        .iter()
        .map(|c| CardRef {
            book: c.addressbook_id.clone(),
            uid: c.uid.clone(),
        })
        .collect();

    let mut found: Vec<Candidate> = Vec::new();
    // One pair, one row: an address *and* a number in common is still one
    // question. First reason wins, and the scan order puts the strongest one
    // first.
    let mut seen: std::collections::HashSet<(CardRef, CardRef)> = std::collections::HashSet::new();

    let mut propose = |a: usize, b: usize, reason: Reason| {
        let (a, b) = (&refs[a], &refs[b]);
        if a == b {
            return;
        }
        // Already one person, or already dismissed.
        if links
            .person_of(&a.book, &a.uid)
            .is_some_and(|person| person.cards.contains(b))
        {
            return;
        }
        if links.is_ignored(a, b) {
            return;
        }
        let key = if a < b {
            (a.clone(), b.clone())
        } else {
            (b.clone(), a.clone())
        };
        if seen.insert(key) {
            found.push(Candidate {
                a: a.clone(),
                b: b.clone(),
                reason,
            });
        }
    };

    // --- exact keys: one pass, no pairwise comparison at all ---
    let mut by_email: HashMap<String, Vec<usize>> = HashMap::new();
    let mut by_phone: HashMap<String, Vec<usize>> = HashMap::new();
    for (index, contact) in contacts.iter().enumerate() {
        for email in &contact.emails {
            let key = email.value.trim().to_lowercase();
            if !key.is_empty() {
                by_email.entry(key).or_default().push(index);
            }
        }
        for phone in &contact.phones {
            if let Some(key) = phone_key(&phone.value) {
                by_phone.entry(key).or_default().push(index);
            }
        }
    }

    for (address, indices) in &by_email {
        for (a, b) in pairs(indices) {
            propose(a, b, Reason::Email(address.clone()));
        }
    }
    for indices in by_phone.values() {
        for (a, b) in pairs(indices) {
            let spelling = contacts[a]
                .phones
                .first()
                .map(|p| p.value.clone())
                .unwrap_or_default();
            propose(a, b, Reason::Phone(spelling));
        }
    }

    // --- fuzzy names, blocked so this is not quadratic over the whole book ---
    //
    // The block is the first letter of the transliterated last token: a
    // surname. Two spellings that disagree on their first letter are missed,
    // which costs a suggestion and saves comparing every card to every other.
    let names: Vec<String> = contacts
        .iter()
        .map(|c| normalise_name(&c.label()))
        .collect();
    let mut by_initial: HashMap<char, Vec<usize>> = HashMap::new();
    for (index, name) in names.iter().enumerate() {
        if let Some(initial) = name
            .split_whitespace()
            .next_back()
            .and_then(|word| word.chars().next())
        {
            by_initial.entry(initial).or_default().push(index);
        }
    }
    for indices in by_initial.values() {
        for (a, b) in pairs(indices) {
            if strsim::jaro_winkler(&names[a], &names[b]) >= NAME_SIMILARITY {
                propose(a, b, Reason::Name);
            }
        }
    }

    found.sort_by_key(|candidate| u8::from(!candidate.reason.is_strong()));
    found
}

/// Every unordered pair of indices in a bucket.
fn pairs(indices: &[usize]) -> Vec<(usize, usize)> {
    let mut out = Vec::new();
    for (offset, a) in indices.iter().enumerate() {
        for b in &indices[offset + 1..] {
            out.push((*a, *b));
        }
    }
    out
}

/// The significant digits of a phone number, or `None` when there are too few
/// to be worth comparing.
#[must_use]
pub fn phone_key(value: &str) -> Option<String> {
    let digits: String = value.chars().filter(char::is_ascii_digit).collect();
    (digits.len() >= PHONE_DIGITS).then(|| digits[digits.len() - PHONE_DIGITS..].to_owned())
}

/// A name reduced to something two spellings of it can agree on:
/// transliterated out of its own script, lowercased, punctuation dropped.
///
/// `Γιώργος Παπάς` and `Giorgos Papas` both land on `giorgos papas`; the
/// remaining distance to `george papas` is what the fuzzy match is for.
#[must_use]
pub fn normalise_name(name: &str) -> String {
    let transliterated = any_ascii::any_ascii(name).to_lowercase();
    let cleaned: String = transliterated
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use cosmic_pim_core::model::Typed;

    fn contact(book: &str, uid: &str, name: &str) -> Contact {
        let mut c = Contact::draft(book);
        c.uid = uid.into();
        c.display_name = name.into();
        c
    }

    fn with_email(mut c: Contact, address: &str) -> Contact {
        c.emails.push(Typed {
            value: address.into(),
            types: Vec::new(),
            pref: None,
            group: None,
            params: Vec::new(),
        });
        c
    }

    fn with_phone(mut c: Contact, number: &str) -> Contact {
        c.phones.push(Typed {
            value: number.into(),
            types: Vec::new(),
            pref: None,
            group: None,
            params: Vec::new(),
        });
        c
    }

    fn card(book: &str, uid: &str) -> CardRef {
        CardRef {
            book: book.into(),
            uid: uid.into(),
        }
    }

    #[test]
    fn one_address_in_two_books_is_a_strong_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let links = LinkStore::open(dir.path());
        let found = candidates(
            &[
                with_email(contact("personal", "a", "Ada Lovelace"), "ada@example.org"),
                with_email(contact("work", "b", "A. Lovelace"), "ADA@Example.org"),
            ],
            &links,
        );
        assert_eq!(found.len(), 1);
        assert!(found[0].reason.is_strong());
    }

    #[test]
    fn a_number_matches_across_spellings() {
        let dir = tempfile::tempdir().unwrap();
        let links = LinkStore::open(dir.path());
        let found = candidates(
            &[
                with_phone(contact("personal", "a", "Ada"), "+1 (555) 123-4567"),
                with_phone(contact("work", "b", "Ada L"), "555-123-4567"),
            ],
            &links,
        );
        assert_eq!(
            found.len(),
            1,
            "the same number spelled two ways was missed"
        );
        assert!(matches!(found[0].reason, Reason::Phone(_)));
    }

    #[test]
    fn a_transliterated_name_is_a_weak_candidate() {
        let dir = tempfile::tempdir().unwrap();
        let links = LinkStore::open(dir.path());
        let found = candidates(
            &[
                contact("personal", "a", "Γιώργος Παπάς"),
                contact("work", "b", "Giorgos Papas"),
            ],
            &links,
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].reason, Reason::Name);
        assert!(
            !found[0].reason.is_strong(),
            "a name match must not claim to be evidence"
        );
    }

    #[test]
    fn unrelated_people_are_not_proposed() {
        let dir = tempfile::tempdir().unwrap();
        let links = LinkStore::open(dir.path());
        let found = candidates(
            &[
                with_email(contact("personal", "a", "Ada Lovelace"), "ada@example.org"),
                with_email(contact("work", "b", "Alan Turing"), "alan@example.org"),
            ],
            &links,
        );
        assert!(found.is_empty(), "proposed two unrelated contacts");
    }

    #[test]
    fn a_pair_already_linked_is_not_proposed_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .link(vec![card("personal", "a"), card("work", "b")])
            .unwrap();

        let found = candidates(
            &[
                with_email(contact("personal", "a", "Ada"), "ada@example.org"),
                with_email(contact("work", "b", "Ada L"), "ada@example.org"),
            ],
            &links,
        );
        assert!(
            found.is_empty(),
            "asked about a pair that is already one person"
        );
    }

    #[test]
    fn a_dismissed_pair_is_not_proposed_again() {
        let dir = tempfile::tempdir().unwrap();
        let mut links = LinkStore::open(dir.path());
        links
            .ignore(card("personal", "a"), card("work", "b"))
            .unwrap();

        let found = candidates(
            &[
                with_email(contact("personal", "a", "Ada"), "ada@example.org"),
                with_email(contact("work", "b", "Someone Else"), "ada@example.org"),
            ],
            &links,
        );
        assert!(
            found.is_empty(),
            "re-asked a question the user has answered"
        );
    }

    /// An address and a number in common is still one question.
    #[test]
    fn two_signals_for_one_pair_make_one_row() {
        let dir = tempfile::tempdir().unwrap();
        let links = LinkStore::open(dir.path());
        let found = candidates(
            &[
                with_phone(
                    with_email(contact("personal", "a", "Ada"), "ada@example.org"),
                    "555-123-4567",
                ),
                with_phone(
                    with_email(contact("work", "b", "Ada L"), "ada@example.org"),
                    "555-123-4567",
                ),
            ],
            &links,
        );
        assert_eq!(found.len(), 1);
    }

    #[test]
    fn short_numbers_are_never_keyed() {
        assert_eq!(phone_key("101"), None);
        assert!(phone_key("555-1234567").is_some());
    }

    #[test]
    fn normalising_strips_script_case_and_punctuation() {
        assert_eq!(normalise_name("Γιώργος Παπάς"), "giorgos papas");
        assert_eq!(normalise_name("O'Brien, Ada"), "o brien ada");
    }
}
