// SPDX-License-Identifier: GPL-3.0-only

//! A `.vcf` holding several cards, edited through Circle's own paths.
//!
//! Every other fixture in this suite is one card per file, which is the shape
//! `vdirsyncer` writes and the shape Circle's own import produces — and it is
//! exactly why a whole class of bug survived in the write path unseen. A file
//! holding many cards is what every export from Google, Apple and Outlook is,
//! and a user who drops one into an address-book directory gets one.
//!
//! The failure mode is not a lost field or a lost record. It is a record
//! wearing somebody else's identity, which is worse than either because
//! nothing about it looks broken: the address book simply has the wrong name
//! on a contact.
//!
//! So every write Circle can perform is checked here against the same
//! two-card file, and each check asserts the same thing — **the card that was
//! not edited is byte-for-byte what it was.**

use cosmic_pim_core::model::{CalendarMeta, Contact, Rgb};
use cosmic_pim_core::store::contacts::{ContactStore, write_contact_raw};

/// Two people in one file, the way an export writes them. Ada carries
/// properties Circle does not model, so "untouched" means something.
const TWO_CARDS: &str = "BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:ada@export\r\n\
FN:Ada Lovelace\r\n\
N:Lovelace;Ada;Augusta;;\r\n\
EMAIL;TYPE=INTERNET:ada@example.org\r\n\
TEL;TYPE=CELL:+30 694 1234567\r\n\
X-ADA-ONLY:kept\r\n\
END:VCARD\r\n\
BEGIN:VCARD\r\n\
VERSION:3.0\r\n\
UID:charles@export\r\n\
FN:Charles Babbage\r\n\
N:Babbage;Charles;;;\r\n\
EMAIL;TYPE=INTERNET:charles@example.org\r\n\
X-CHARLES-ONLY:also kept\r\n\
END:VCARD\r\n";

struct Fixture {
    _dir: tempfile::TempDir,
    store: ContactStore,
    book: CalendarMeta,
}

fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("scratch directory");
    let mut store = ContactStore::open(&dir.path().join("contacts")).expect("open the store");
    let book = store
        .create_book("Imported", Rgb(0x84, 0x2b, 0xd2))
        .expect("create a book");
    write_contact_raw(&book, "export.vcf", TWO_CARDS).expect("seed the export");
    store.refresh();
    Fixture {
        _dir: dir,
        store,
        book,
    }
}

impl Fixture {
    fn contact(&self, uid: &str) -> Contact {
        self.store
            .contacts()
            .into_iter()
            .find(|c| c.uid == uid)
            .unwrap_or_else(|| panic!("no contact with uid {uid}"))
    }

    /// The file on disk, whole.
    fn file(&self) -> String {
        std::fs::read_to_string(self.book.path.join("export.vcf")).expect("the export file")
    }

    /// One card's text, sliced out of the file by its UID.
    fn card(&self, uid: &str) -> String {
        let text = self.file();
        text.split("BEGIN:VCARD")
            .find(|part| part.contains(&format!("UID:{uid}")))
            .map(|part| format!("BEGIN:VCARD{part}"))
            .unwrap_or_else(|| panic!("no card for {uid} in the file"))
    }
}

/// Both people are read out of the one file, each as themselves.
#[test]
fn a_multi_card_file_reads_as_several_contacts() {
    let fixture = fixture();
    let contacts = fixture.store.contacts();
    assert_eq!(contacts.len(), 2, "{contacts:#?}");
    assert_eq!(fixture.contact("ada@export").label(), "Ada Lovelace");
    assert_eq!(fixture.contact("charles@export").label(), "Charles Babbage");
    // Both carry the same `raw` — the whole file — which is the thing that
    // made every write path below dangerous.
    assert_eq!(
        fixture.contact("ada@export").raw,
        fixture.contact("charles@export").raw,
        "the fixture stopped covering the shared-raw shape"
    );
}

/// The reported bug: editing the second card wrote over the first.
#[test]
fn editing_the_second_contact_leaves_the_first_untouched() {
    let mut fixture = fixture();
    let ada_before = fixture.card("ada@export");

    let mut charles = fixture.contact("charles@export");
    charles.display_name = "Charles Babbage FRS".into();
    fixture.store.save(&charles).expect("save Charles");

    assert_eq!(
        fixture.card("ada@export"),
        ada_before,
        "editing Charles rewrote Ada's card"
    );
    let after = fixture.card("charles@export");
    assert!(
        after.contains("FN:Charles Babbage FRS"),
        "the edit never reached Charles: {after}"
    );
    assert!(
        after.contains("X-CHARLES-ONLY:also kept"),
        "the edit dropped Charles's unmodelled properties: {after}"
    );
}

/// The same, the other way round — the first card is not a special case.
#[test]
fn editing_the_first_contact_leaves_the_second_untouched() {
    let mut fixture = fixture();
    let charles_before = fixture.card("charles@export");

    let mut ada = fixture.contact("ada@export");
    ada.display_name = "Ada, Countess of Lovelace".into();
    fixture.store.save(&ada).expect("save Ada");

    assert_eq!(
        fixture.card("charles@export"),
        charles_before,
        "editing Ada rewrote Charles's card"
    );
    assert!(fixture.card("ada@export").contains("Countess"));
}

/// A contact whose card is not in the file must be refused, not guessed into
/// somebody else's record.
#[test]
fn saving_a_contact_absent_from_the_file_is_refused() {
    let mut fixture = fixture();
    let before = fixture.file();

    let mut stranger = fixture.contact("ada@export");
    stranger.uid = "nobody@export".into();
    stranger.display_name = "Not In This File".into();

    let result = fixture.store.save(&stranger);
    assert!(
        result.is_err(),
        "a contact absent from a multi-card file was written anyway"
    );
    assert_eq!(
        fixture.file(),
        before,
        "the refused save still changed the file"
    );
}

/// Deleting one person out of a shared file must not take the others with
/// them.
#[test]
fn deleting_one_contact_leaves_the_others_in_the_file() {
    let mut fixture = fixture();
    let ada_before = fixture.card("ada@export");

    fixture
        .store
        .delete(&fixture.book.id, "charles@export")
        .expect("delete Charles");
    fixture.store.refresh();

    let remaining = fixture.store.contacts();
    assert_eq!(
        remaining.len(),
        1,
        "deleting one contact from a shared file removed {} of them",
        2 - remaining.len().min(2)
    );
    assert_eq!(
        remaining[0].uid, "ada@export",
        "the wrong person was deleted"
    );
    assert_eq!(
        fixture.card("ada@export"),
        ada_before,
        "Ada's card changed when Charles was deleted"
    );
}

/// Re-importing the export a file came from must update the people in it, not
/// leave one of them.
///
/// This is the promise the README makes about import — "UID-keyed, so
/// re-importing the same export updates rather than duplicates" — and it is
/// made about exactly the file shape an export has.
#[test]
fn reimporting_the_export_keeps_everybody_in_it() {
    let mut fixture = fixture();

    let summary = fixture
        .store
        .import_vcf(TWO_CARDS, &fixture.book.id)
        .expect("re-import the same export");
    fixture.store.refresh();

    assert_eq!(summary.updated, 2, "both cards should have been recognised");
    assert_eq!(summary.added, 0, "a re-import should add nobody");

    let after = fixture.store.contacts();
    assert_eq!(
        after.len(),
        2,
        "re-importing a two-card export left {} of the two people",
        after.len()
    );
    assert!(fixture.card("ada@export").contains("X-ADA-ONLY:kept"));
    assert!(
        fixture
            .card("charles@export")
            .contains("X-CHARLES-ONLY:also kept")
    );
}

/// Importing an updated export must change the person it names and leave the
/// people it shares a file with alone.
#[test]
fn importing_an_update_for_one_person_keeps_the_other() {
    let mut fixture = fixture();
    let charles_before = fixture.card("charles@export");

    let just_ada = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada@export\r\n\
FN:Ada, Countess of Lovelace\r\nEMAIL;TYPE=INTERNET:ada@example.org\r\nEND:VCARD\r\n";
    fixture
        .store
        .import_vcf(just_ada, &fixture.book.id)
        .expect("import Ada's update");
    fixture.store.refresh();

    assert_eq!(
        fixture.store.contacts().len(),
        2,
        "importing Ada removed Charles"
    );
    assert_eq!(
        fixture.card("charles@export"),
        charles_before,
        "importing Ada rewrote Charles's card"
    );
    assert!(fixture.card("ada@export").contains("Countess"));
}

/// Two people whose UIDs differ only where the file-name sanitiser is lossy
/// must stay two people.
///
/// `a@x.com` and `a-x.com` both clean to `a-x.com`, so before the sanitiser
/// carried a digest they named one file. The second import then landed in the
/// first's file and the result was one record with the first's UID and the
/// second's content — the same identity swap as the patcher bug, reached
/// through a different door.
///
/// The same shape bites any two UIDs sharing a 120-character prefix, which is
/// what a server that mints long opaque ids produces.
#[test]
fn uids_that_sanitise_alike_stay_separate_contacts() {
    let dir = tempfile::tempdir().expect("scratch directory");
    let mut store = ContactStore::open(&dir.path().join("contacts")).expect("open the store");
    let book = store
        .create_book("Imported", Rgb(0x84, 0x2b, 0xd2))
        .expect("create a book");

    let colliding = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a@x.com\r\nFN:First Person\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nUID:a-x.com\r\nFN:Second Person\r\nEND:VCARD\r\n";

    let summary = store.import_vcf(colliding, &book.id).expect("import");
    store.refresh();

    assert_eq!(summary.added, 2, "both cards should have been added");
    let files: std::collections::HashSet<&String> = summary.files.iter().collect();
    assert_eq!(
        files.len(),
        2,
        "the two uids were written to one file: {:?}",
        summary.files
    );

    let contacts = store.contacts();
    assert_eq!(contacts.len(), 2, "the import merged two people into one");

    // Each record must carry its OWN name, not the other's. A count of two is
    // not enough — the failure this guards against is a record keeping one
    // uid while taking the other's content.
    let first = contacts
        .iter()
        .find(|c| c.uid == "a@x.com")
        .expect("the first uid survived");
    let second = contacts
        .iter()
        .find(|c| c.uid == "a-x.com")
        .expect("the second uid survived");
    assert_eq!(
        first.label(),
        "First Person",
        "the first record took the second's content"
    );
    assert_eq!(
        second.label(),
        "Second Person",
        "the second record took the first's content"
    );
}

/// Setting a photo goes through a different patcher than the field editor.
#[test]
fn setting_a_photo_on_one_contact_leaves_the_other_untouched() {
    let fixture = fixture();
    let ada_before = fixture.card("ada@export");

    let charles = fixture.contact("charles@export");
    let patched =
        cosmic_pim_core::vcard::set_photo(&charles.raw, &charles.uid, b"\x89PNG-ish", "image/png")
            .expect("patch a photo in");
    write_contact_raw(&fixture.book, &charles.file_name, &patched).expect("write it back");

    assert_eq!(
        fixture.card("ada@export"),
        ada_before,
        "setting Charles's photo wrote it onto Ada's card"
    );
    assert!(
        fixture.card("charles@export").contains("PHOTO"),
        "the photo never reached Charles"
    );
}

/// Group membership is a third patcher again.
#[test]
fn setting_members_on_one_card_leaves_the_other_untouched() {
    let fixture = fixture();
    let ada_before = fixture.card("ada@export");

    let charles = fixture.contact("charles@export");
    let patched = cosmic_pim_core::vcard::set_members(
        &charles.raw,
        &charles.uid,
        &[cosmic_pim_core::vcard::member_uri("someone@else")],
    )
    .expect("patch members in");
    write_contact_raw(&fixture.book, &charles.file_name, &patched).expect("write it back");

    assert_eq!(
        fixture.card("ada@export"),
        ada_before,
        "setting members on Charles wrote them onto Ada's card"
    );
}
