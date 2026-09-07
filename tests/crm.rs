// SPDX-License-Identifier: GPL-3.0-only

//! The CRM layer against real files, and the two promises it makes that no
//! unit test can show on its own.
//!
//! **Notes never touch a card.** The whole reason this data lives outside the
//! address books is that a shared book must not grow a field saying when you
//! last rang a colleague. That is a claim about the `.vcf` bytes, so it is
//! checked against them.
//!
//! **The vdir layer cannot see `.crm/`.** A directory that registered as a
//! collection would be listed as an address book and, worse, offered to a
//! CardDAV server — publishing exactly the notes this design keeps private.

use chrono::{Duration, Utc};
use circle::crm::{CrmStore, summarise};
use circle::links::{CardRef, LinkStore};
use cosmic_pim_core::model::Rgb;
use cosmic_pim_core::store::contacts::{ContactStore, write_contact_raw};

const ADA: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:ada@home\r\n\
FN:Ada Lovelace\r\n\
EMAIL;TYPE=INTERNET:ada@example.org\r\n\
X-CUSTOM:untouched\r\n\
END:VCARD\r\n";

const ADA_WORK: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:ada@work\r\n\
FN:A. Lovelace\r\n\
EMAIL;TYPE=INTERNET:ada@example.org\r\n\
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

    let personal = store
        .create_book("Personal", Rgb(0x84, 0x2b, 0xd2))
        .expect("create the personal book");
    let work = store
        .create_book("Work", Rgb(0x2d, 0x7d, 0xd2))
        .expect("create the work book");
    write_contact_raw(&personal, "ada.vcf", ADA).expect("seed the home card");
    write_contact_raw(&work, "ada-work.vcf", ADA_WORK).expect("seed the work card");
    store.refresh();

    Fixture {
        _dir: dir,
        root,
        store,
    }
}

impl Fixture {
    fn card(&self, uid: &str) -> CardRef {
        let contact = self
            .store
            .contacts()
            .into_iter()
            .find(|c| c.uid == uid)
            .unwrap_or_else(|| panic!("no card with uid {uid}"));
        CardRef {
            book: contact.addressbook_id,
            uid: contact.uid,
        }
    }

    fn raw(&self, uid: &str) -> String {
        self.store
            .contacts()
            .into_iter()
            .find(|c| c.uid == uid)
            .expect("the card")
            .raw
    }
}

/// The promise the whole design rests on.
#[test]
fn notes_and_cadences_never_reach_the_card() {
    let fixture = fixture();
    let before = fixture.raw("ada@home");
    let ada = fixture.card("ada@home");

    let mut crm = CrmStore::open(&fixture.root);
    crm.add_note(&ada, "Owes me a lyre lesson", Utc::now())
        .expect("add a note");
    crm.log_interaction(&ada, "called", Utc::now())
        .expect("log a contact");
    crm.set_cadence(&ada, Some(30)).expect("set a cadence");

    let mut store = ContactStore::open(&fixture.root).expect("reopen");
    store.refresh();
    let after = store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@home")
        .expect("the card is still there")
        .raw;

    assert_eq!(after, before, "the CRM layer rewrote the contact's card");
    assert!(
        !after.contains("lyre") && !after.contains("30"),
        "a note or a cadence leaked into the card: {after}"
    );
    assert!(
        before.contains("X-CUSTOM:untouched"),
        "the fixture stopped covering unmodelled properties"
    );
}

#[test]
fn the_crm_store_never_becomes_an_address_book() {
    let fixture = fixture();
    let ada = fixture.card("ada@home");
    CrmStore::open(&fixture.root)
        .add_note(&ada, "Private", Utc::now())
        .expect("add a note");

    let mut store = ContactStore::open(&fixture.root).expect("reopen");
    store.refresh();
    assert_eq!(
        store.books().len(),
        2,
        "the CRM store was picked up as an address book: {:?}",
        store.books().iter().map(|b| &b.name).collect::<Vec<_>>()
    );
}

/// Linking unions history the same way it unions addresses, and unlinking
/// takes each card's own notes back out with it.
#[test]
fn linking_unions_history_and_unlinking_returns_it() {
    let fixture = fixture();
    let (home, work) = (fixture.card("ada@home"), fixture.card("ada@work"));

    let mut crm = CrmStore::open(&fixture.root);
    crm.add_note(&home, "Birthday drinks", Utc::now() - Duration::days(10))
        .expect("home note");
    crm.add_note(
        &work,
        "Reviewed the proposal",
        Utc::now() - Duration::days(2),
    )
    .expect("work note");

    let mut links = LinkStore::open(&fixture.root);
    links
        .link(vec![home.clone(), work.clone()])
        .expect("link the pair");

    // As one person: both notes, newest first.
    let person = summarise(&crm, &[home.clone(), work.clone()]);
    assert_eq!(person.notes.len(), 2);
    assert_eq!(person.notes[0].text, "Reviewed the proposal");

    // Unlinked again: each card keeps only its own.
    links.unlink(&work.book, &work.uid).expect("unlink");
    assert_eq!(summarise(&crm, &[home]).notes.len(), 1);
    assert_eq!(summarise(&crm, &[work]).notes.len(), 1);
}

/// Overdue is a question about today, asked of the data — not a flag stored
/// on anything.
#[test]
fn a_cadence_falls_due_and_logging_a_contact_clears_it() {
    let fixture = fixture();
    let ada = fixture.card("ada@home");
    let mut crm = CrmStore::open(&fixture.root);

    crm.set_cadence(&ada, Some(30)).expect("set a cadence");
    crm.log_interaction(&ada, "called", Utc::now() - Duration::days(45))
        .expect("an old contact");
    assert!(
        summarise(&crm, std::slice::from_ref(&ada)).is_overdue(Utc::now()),
        "45 days past a 30-day cadence should be overdue"
    );

    crm.log_interaction(&ada, "called", Utc::now())
        .expect("a fresh contact");
    assert!(
        !summarise(&crm, std::slice::from_ref(&ada)).is_overdue(Utc::now()),
        "logging a contact should clear the overdue state"
    );
}

/// The blob a scan is stored as is shared between everyone it is attached to,
/// and survives being detached from one of them.
#[test]
fn one_scan_attached_to_two_people_is_stored_once_and_outlives_one_detach() {
    let fixture = fixture();
    let (home, work) = (fixture.card("ada@home"), fixture.card("ada@work"));

    let scan = fixture._dir.path().join("business-card.png");
    std::fs::write(&scan, b"a scanned business card").expect("write the scan");

    let mut crm = CrmStore::open(&fixture.root);
    let stored = circle::attachments::store(&fixture.root, &scan).expect("store the blob");
    crm.attach(&home, stored.clone()).expect("attach to home");
    crm.attach(&work, stored.clone()).expect("attach to work");

    let blobs = std::fs::read_dir(circle::attachments::blob_dir(&fixture.root))
        .expect("the blob directory")
        .count();
    assert_eq!(blobs, 1, "the same bytes were stored twice");

    // Detached from one: still referenced by the other, so the file stays.
    crm.detach(&home, &stored.blob).expect("detach from home");
    assert!(crm.is_blob_referenced(&stored.blob));
    circle::attachments::prune(&fixture.root, &stored.blob, true).expect("prune");
    assert!(
        circle::attachments::path(&fixture.root, &stored).exists(),
        "deleted a blob the other card still points at"
    );

    // Detached from both: nothing references it, so it goes.
    crm.detach(&work, &stored.blob).expect("detach from work");
    assert!(!crm.is_blob_referenced(&stored.blob));
    circle::attachments::prune(&fixture.root, &stored.blob, false).expect("prune");
    assert!(!circle::attachments::path(&fixture.root, &stored).exists());
}

/// Attachments are the heaviest thing here, so the promise that nothing
/// reaches the card matters most for them.
#[test]
fn an_attachment_never_reaches_the_card() {
    let fixture = fixture();
    let before = fixture.raw("ada@home");
    let ada = fixture.card("ada@home");

    let scan = fixture._dir.path().join("contract.pdf");
    std::fs::write(&scan, vec![0u8; 4096]).expect("write the file");

    let mut crm = CrmStore::open(&fixture.root);
    let stored = circle::attachments::store(&fixture.root, &scan).expect("store");
    crm.attach(&ada, stored).expect("attach");

    let mut store = ContactStore::open(&fixture.root).expect("reopen");
    store.refresh();
    let after = store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@home")
        .expect("the card")
        .raw;

    assert_eq!(after, before, "attaching a file rewrote the card");
    assert_eq!(
        store.books().len(),
        2,
        "the blob directory registered as an address book"
    );
}

/// Notes survive the process that wrote them, because they are files.
#[test]
fn everything_survives_reopening_the_store() {
    let fixture = fixture();
    let ada = fixture.card("ada@home");
    {
        let mut crm = CrmStore::open(&fixture.root);
        crm.add_note(&ada, "Met at the conference", Utc::now())
            .expect("add a note");
        crm.set_cadence(&ada, Some(90)).expect("set a cadence");
        crm.log_interaction(&ada, "coffee", Utc::now())
            .expect("log a contact");
    }

    let reopened = CrmStore::open(&fixture.root);
    let summary = summarise(&reopened, &[ada]);
    assert_eq!(summary.notes.len(), 1);
    assert_eq!(summary.cadence_days, Some(90));
    assert!(summary.last_contacted.is_some());
}
