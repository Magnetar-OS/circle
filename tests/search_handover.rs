// SPDX-License-Identifier: GPL-3.0-only

//! What `--search=` hands over, and when that is enough to pick a person.
//!
//! The launcher plugin's promise is that Enter on a row opens **that person**.
//! It passes `--search=<their name>` over the single-instance bus, so whether
//! the promise is kept comes down to one question: does that query name one
//! contact, or several? This pins the rule the shell applies — one match is
//! unambiguous and gets selected, more than one is a list the user still
//! chooses from — against the same substrate search the shell calls.

use cosmic_pim_core::model::Rgb;
use cosmic_pim_core::store::contacts::{ContactStore, write_contact_raw};

fn card(uid: &str, name: &str, email: &str) -> String {
    format!(
        "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:{uid}\r\nFN:{name}\r\n\
         EMAIL;TYPE=INTERNET:{email}\r\nEND:VCARD\r\n"
    )
}

fn fixture() -> (tempfile::TempDir, ContactStore) {
    let dir = tempfile::tempdir().expect("scratch directory");
    let mut store = ContactStore::open(&dir.path().join("contacts")).expect("open the store");
    let book = store
        .create_book("Personal", Rgb(0x84, 0x2b, 0xd2))
        .expect("create a book");

    for (uid, name, email) in [
        ("ada", "Ada Lovelace", "ada@example.org"),
        ("alan", "Alan Turing", "alan@example.org"),
        ("grace", "Grace Hopper", "grace@example.org"),
    ] {
        write_contact_raw(&book, &format!("{uid}.vcf"), &card(uid, name, email))
            .expect("seed a card");
    }
    store.refresh();
    (dir, store)
}

/// The launcher's own case: it passes the label of the row that was pressed,
/// so the query names one person and the shell can open them.
#[test]
fn a_query_naming_one_person_matches_exactly_one() {
    let (_dir, store) = fixture();
    let found = store.search("Ada Lovelace");
    assert_eq!(found.len(), 1, "expected one match, got {found:?}");
    assert_eq!(found[0].uid, "ada");
}

/// A query several people answer to must stay a list. Selecting the first
/// would be a guess, and a wrong guess here opens the wrong person's details.
#[test]
fn an_ambiguous_query_matches_more_than_one() {
    let (_dir, store) = fixture();
    assert!(
        store.search("a").len() > 1,
        "the fixture stopped being ambiguous, so this no longer tests anything"
    );
}

/// A query nothing answers to selects nobody rather than falling back to
/// whoever happens to be first.
#[test]
fn a_query_matching_nobody_matches_nothing() {
    let (_dir, store) = fixture();
    assert!(store.search("Nobody At All").is_empty());
}
