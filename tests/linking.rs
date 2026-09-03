// SPDX-License-Identifier: GPL-3.0-only

//! Linking, end to end: two real cards in two real books on disk, found as
//! duplicates, linked into one person, composed into one detail view, and
//! unlinked back into two intact cards.
//!
//! The unit tests cover each piece — the store's records, the finder's
//! signals, the composer's precedence. What they cannot show is the promise
//! the feature actually makes: **linking is not merging.** The cards on disk
//! must be byte-for-byte what they were, before and after, so each one goes on
//! syncing to its own server unchanged and unlinking loses nothing. That is
//! what this file pins.

use circle::dedupe::{self, Reason};
use circle::links::{CardRef, LinkStore};
use circle::ui::person;
use cosmic_pim_core::model::{Contact, Rgb};
use cosmic_pim_core::store::contacts::{ContactStore, write_contact_raw};

/// The same person from two accounts: one shared address, and each card
/// carrying properties the other does not — including ones Circle does not
/// model, which is what makes "intact" worth asserting.
const WORK: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:ada@work-server\r\n\
FN:Ada Lovelace\r\n\
N:Lovelace;Ada;;;\r\n\
EMAIL;TYPE=INTERNET:ada@example.org\r\n\
TEL;TYPE=WORK:+30 210 1234567\r\n\
ORG:Analytical Engine Co\r\n\
X-WORK-ONLY:kept\r\n\
END:VCARD\r\n";

const HOME: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@home-server\r\n\
FN:Ada\r\n\
N:Lovelace;Ada;;;\r\n\
EMAIL:ADA@Example.org\r\n\
TEL:+30 694 7654321\r\n\
BDAY:18151210\r\n\
X-HOME-ONLY:also kept\r\n\
END:VCARD\r\n";

struct Fixture {
    _dir: tempfile::TempDir,
    root: std::path::PathBuf,
    store: ContactStore,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("scratch directory");
    let root = dir.path().join("contacts");
    let mut store = ContactStore::open(&root).expect("open the store");

    let work = store
        .create_book("Work", Rgb(0x84, 0x2b, 0xd2))
        .expect("create the work book");
    let home = store
        .create_book("Personal", Rgb(0x2b, 0x84, 0xd2))
        .expect("create the personal book");
    write_contact_raw(&work, "ada-work.vcf", WORK).expect("seed the work card");
    write_contact_raw(&home, "ada-home.vcf", HOME).expect("seed the home card");
    store.refresh();

    Fixture {
        _dir: dir,
        root,
        store,
    }
}

impl Fixture {
    fn contacts(&self) -> Vec<Contact> {
        self.store.contacts()
    }

    fn card(&self, uid: &str) -> Contact {
        self.contacts()
            .into_iter()
            .find(|c| c.uid == uid)
            .unwrap_or_else(|| panic!("no card with uid {uid}"))
    }

    fn refs(&self) -> (CardRef, CardRef) {
        let work = self.card("ada@work-server");
        let home = self.card("ada@home-server");
        (
            CardRef {
                book: work.addressbook_id,
                uid: work.uid,
            },
            CardRef {
                book: home.addressbook_id,
                uid: home.uid,
            },
        )
    }
}

/// The whole journey, in the order a user walks it.
#[test]
fn two_accounts_become_one_person_and_come_apart_again_unchanged() {
    let fixture = fixture();
    let before_work = fixture.card("ada@work-server").raw;
    let before_home = fixture.card("ada@home-server").raw;
    let mut links = LinkStore::open(&fixture.root);

    // 1. The finder proposes them, on the address they share.
    let contacts = fixture.contacts();
    let found = dedupe::candidates(&contacts, &links);
    assert_eq!(found.len(), 1, "the shared address did not surface a pair");
    assert!(
        matches!(found[0].reason, Reason::Email(_)),
        "expected the address to be the evidence, got {:?}",
        found[0].reason
    );

    // 2. Linking them writes only Circle's own metadata.
    let (work, home) = fixture.refs();
    links
        .link(vec![work.clone(), home.clone()])
        .expect("link the pair");
    assert_eq!(
        fixture.card("ada@work-server").raw,
        before_work,
        "linking rewrote the work card"
    );
    assert_eq!(
        fixture.card("ada@home-server").raw,
        before_home,
        "linking rewrote the home card"
    );

    // 3. Both cards now answer to one person, work first — the order they
    //    were linked in, which is the precedence the composer honours.
    let person_record = links
        .person_of(&work.book, &work.uid)
        .expect("the work card belongs to a person");
    assert_eq!(person_record.cards.len(), 2);
    assert_eq!(person_record.cards[0], work);
    assert!(
        links.person_of(&home.book, &home.uid).is_some(),
        "the home card was left out of its own person"
    );

    // 4. Composed, they are one entry: the shared address once, both numbers,
    //    each attributed, and the birthday only the home card carries.
    let work_card = fixture.card("ada@work-server");
    let home_card = fixture.card("ada@home-server");
    let cards = [(&work_card, "Work"), (&home_card, "Personal")];
    let composed = person::compose(&cards).expect("compose the person");

    assert!(composed.is_linked());
    assert_eq!(
        composed.label, "Ada Lovelace",
        "the head card names the person"
    );
    let emails: Vec<&str> = composed
        .fields
        .iter()
        .filter(|f| f.icon == "mail-send-symbolic")
        .map(|f| f.value.as_str())
        .collect();
    assert_eq!(
        emails,
        ["ada@example.org"],
        "the shared address should appear once, in the head card's spelling"
    );
    let phones: Vec<&str> = composed
        .fields
        .iter()
        .filter(|f| f.icon == "call-start-symbolic")
        .map(|f| f.value.as_str())
        .collect();
    assert_eq!(phones, ["+30 210 1234567", "+30 694 7654321"]);
    assert!(
        composed.fields.iter().any(|f| f.source == Some("Personal")),
        "nothing was attributed to the second card"
    );
    assert!(
        composed
            .fields
            .iter()
            .any(|f| f.value.contains("1815") && f.source == Some("Personal")),
        "the birthday only the home card carries did not reach the composed view"
    );
    assert_eq!(
        composed.organisation.as_deref(),
        Some("Analytical Engine Co"),
        "the head card's organisation should win"
    );

    // 5. The pair is not proposed a second time.
    assert!(
        dedupe::candidates(&fixture.contacts(), &links).is_empty(),
        "a linked pair was proposed again"
    );

    // 6. Unlinking dissolves the person and leaves both cards exactly as they
    //    were on disk — the promise the whole feature rests on.
    links.unlink(&home.book, &home.uid).expect("unlink");
    assert!(
        links.persons().is_empty(),
        "a one-card person survived the unlink"
    );
    assert_eq!(fixture.card("ada@work-server").raw, before_work);
    assert_eq!(fixture.card("ada@home-server").raw, before_home);
    assert!(
        before_work.contains("X-WORK-ONLY:kept") && before_home.contains("X-HOME-ONLY:also kept"),
        "the fixture stopped covering unmodelled properties"
    );
}

/// The link record survives a restart, because it is a file like everything
/// else — the store the app opens next launch is a fresh one.
#[test]
fn a_link_survives_reopening_the_store() {
    let fixture = fixture();
    let (work, home) = fixture.refs();

    LinkStore::open(&fixture.root)
        .link(vec![work.clone(), home])
        .expect("link the pair");

    let reopened = LinkStore::open(&fixture.root);
    assert_eq!(reopened.persons().len(), 1);
    assert!(reopened.person_of(&work.book, &work.uid).is_some());
}

/// The link store is Circle's own metadata and must stay invisible to the vdir
/// layer: a `.links` directory that registered as an address book would show
/// up in the sidebar and, worse, be offered to a server as a collection.
#[test]
fn the_link_store_never_becomes_an_address_book() {
    let fixture = fixture();
    let (work, home) = fixture.refs();
    LinkStore::open(&fixture.root)
        .link(vec![work, home])
        .expect("link the pair");

    let mut store = ContactStore::open(&fixture.root).expect("reopen the store");
    store.refresh();
    assert_eq!(
        store.books().len(),
        2,
        "the link store was picked up as an address book: {:?}",
        store.books().iter().map(|b| &b.name).collect::<Vec<_>>()
    );
}

/// Dismissing a pair is remembered, so the review screen never re-asks.
#[test]
fn a_dismissed_pair_stays_dismissed_across_restarts() {
    let fixture = fixture();
    let (work, home) = fixture.refs();

    LinkStore::open(&fixture.root)
        .ignore(work, home)
        .expect("dismiss the pair");

    let reopened = LinkStore::open(&fixture.root);
    assert!(
        dedupe::candidates(&fixture.contacts(), &reopened).is_empty(),
        "a dismissed pair came back after a restart"
    );
}
