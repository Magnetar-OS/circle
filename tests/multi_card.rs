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
