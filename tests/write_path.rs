// SPDX-License-Identifier: GPL-3.0-only

//! The write path, end to end: the editor's model → the substrate's patcher →
//! bytes on disk → parsed back.
//!
//! The unit tests in `ui::editor` cover the editor's own bookkeeping and the
//! substrate's cover the patcher. Neither proves the two are wired to each
//! other, and that join is exactly where this app's central promise lives:
//! editing a synced contact must not destroy the parts of the card Circle does
//! not display. A regression here is silent — the app looks right, and the loss
//! only becomes visible on another device after the next sync.

use circle::ui::editor::{Field, ListKind, Message, State};
use cosmic_pim_core::model::{CalendarMeta, Contact, Rgb};
use cosmic_pim_core::store::contacts::{ContactStore, write_contact_raw};

/// A card of the kind a real server sends: more properties than Circle models,
/// an Apple-grouped address with a custom label, and a photo.
const SYNCED: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@server\r\n\
FN:Ada Lovelace\r\n\
N:Lovelace;Ada;Augusta;;\r\n\
EMAIL;TYPE=work:ada@work.example\r\n\
item1.EMAIL;type=INTERNET:ada@home.example\r\n\
item1.X-ABLabel:Summer house\r\n\
TEL;TYPE=voice:+30 210 1234567\r\n\
PHOTO;ENCODING=b:AAAABBBBCCCC\r\n\
GEO:geo:37.98,23.72\r\n\
X-ABShowAs:COMPANY\r\n\
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
        .create_book("Personal", Rgb(0x84, 0x2b, 0xd2))
        .expect("create a book");
    write_contact_raw(&book, "ada.vcf", SYNCED).expect("seed a synced card");
    store.refresh();
    Fixture {
        _dir: dir,
        store,
        book,
    }
}

impl Fixture {
    fn ada(&self) -> Contact {
        self.store
            .contacts()
            .into_iter()
            .find(|c| c.uid == "ada@server")
            .expect("the seeded contact")
    }

    fn on_disk(&self) -> String {
        std::fs::read_to_string(self.book.path.join("ada.vcf")).expect("read the card back")
    }

    fn editor(&self) -> State {
        State::edit(self.ada(), self.store.books())
    }
}

#[test]
fn editing_a_synced_contact_through_the_editor_keeps_everything_it_does_not_model() {
    let mut fixture = fixture();

    let mut editor = fixture.editor();
    editor.update(Message::Text(Field::Family, "Byron".into()));
    editor.update(Message::Text(Field::DisplayName, "Ada Byron".into()));
    editor.update(Message::Text(
        Field::Organisation,
        "Analytical Engines".into(),
    ));

    fixture.store.save(&editor.finish()).expect("save");

    let card = fixture.on_disk();
    assert!(
        card.contains("FN:Ada Byron"),
        "the edit did not land:\n{card}"
    );
    assert!(card.contains("ORG:Analytical Engines"), "{card}");

    // The whole point.
    assert!(
        card.contains("PHOTO;ENCODING=b:AAAABBBBCCCC"),
        "a name change destroyed the photo:\n{card}"
    );
    assert!(
        card.contains("GEO:geo:37.98\\,23.72") || card.contains("GEO:geo:37.98,23.72"),
        "{card}"
    );
    assert!(card.contains("X-ABShowAs:COMPANY"), "{card}");
    assert!(card.contains("item1.X-ABLabel:Summer house"), "{card}");
}

#[test]
fn a_grouped_address_is_not_duplicated_by_repeated_saves() {
    let mut fixture = fixture();

    // Three round trips through the editor. The bug this guards against added
    // one ungrouped copy of the grouped address per save.
    for _ in 0..3 {
        let editor = fixture.editor();
        fixture.store.save(&editor.finish()).expect("save");
    }

    let card = fixture.on_disk();
    assert_eq!(
        card.matches("ada@home.example").count(),
        1,
        "the grouped address was duplicated:\n{card}"
    );
    assert_eq!(fixture.ada().emails.len(), 2);
}

#[test]
fn editing_a_grouped_value_rewrites_it_in_place_and_keeps_its_label() {
    let mut fixture = fixture();

    let mut editor = fixture.editor();
    let grouped = editor
        .contact
        .emails
        .iter()
        .position(|e| e.is_grouped())
        .expect("the seeded card has a grouped address");
    editor.update(Message::ListValue(
        ListKind::Email,
        grouped,
        "ada@villa.example".into(),
    ));

    fixture.store.save(&editor.finish()).expect("save");

    let card = fixture.on_disk();
    assert!(card.contains("ada@villa.example"), "{card}");
    assert!(
        !card.contains("ada@home.example"),
        "the old value survived:\n{card}"
    );
    assert!(
        card.contains("item1.X-ABLabel:Summer house"),
        "the custom label was orphaned:\n{card}"
    );
    assert_eq!(
        card.matches("ada@villa.example").count(),
        1,
        "the edit was written as a second, ungrouped line:\n{card}"
    );
}

#[test]
fn adding_and_removing_ungrouped_values_lands_on_disk() {
    let mut fixture = fixture();

    let mut editor = fixture.editor();
    editor.update(Message::ListAdd(ListKind::Email));
    let added = editor.contact.emails.len() - 1;
    editor.update(Message::ListValue(
        ListKind::Email,
        added,
        "ada@new.example".into(),
    ));
    editor.update(Message::ListPreferred(ListKind::Email, added));
    fixture.store.save(&editor.finish()).expect("save");

    let back = fixture.ada();
    assert_eq!(back.emails.len(), 3);
    assert_eq!(
        Contact::preferred(&back.emails).map(|e| e.value.as_str()),
        Some("ada@new.example"),
        "PREF did not survive the round trip"
    );

    // Now remove the ungrouped work address.
    let mut editor = State::edit(back, fixture.store.books());
    let work = editor
        .contact
        .emails
        .iter()
        .position(|e| e.value == "ada@work.example")
        .expect("the work address");
    editor.update(Message::ListRemove(ListKind::Email, work));
    fixture.store.save(&editor.finish()).expect("save");

    let card = fixture.on_disk();
    assert!(!card.contains("ada@work.example"), "{card}");
    assert!(
        card.contains("item1.EMAIL"),
        "removing an ungrouped address took the grouped one with it:\n{card}"
    );
    assert!(card.contains("PHOTO;ENCODING=b:AAAABBBBCCCC"), "{card}");
}

#[test]
fn a_contact_created_in_the_editor_round_trips() {
    let mut fixture = fixture();

    let mut editor = State::create(&fixture.book.id, fixture.store.books());
    editor.update(Message::Text(Field::Given, "Alan".into()));
    editor.update(Message::Text(Field::Family, "Turing".into()));
    editor.update(Message::ListAdd(ListKind::Phone));
    editor.update(Message::ListValue(
        ListKind::Phone,
        0,
        "+44 20 7946 0000".into(),
    ));
    editor.update(Message::Text(Field::Birthday, "1912-06-23".into()));
    editor.update(Message::Text(Field::Categories, "Friends, Maths".into()));

    let created = editor.finish();
    fixture.store.save(&created).expect("save");

    let back = fixture
        .store
        .contacts()
        .into_iter()
        .find(|c| c.uid == created.uid)
        .expect("the new contact");

    assert_eq!(back.label(), "Alan Turing");
    assert_eq!(back.phones.len(), 1);
    assert_eq!(back.birthday, chrono::NaiveDate::from_ymd_opt(1912, 6, 23));
    assert_eq!(back.categories, vec!["Friends", "Maths"]);
}

#[test]
fn deleting_removes_the_file_and_leaves_the_rest_of_the_book_alone() {
    let mut fixture = fixture();

    let mut editor = State::create(&fixture.book.id, fixture.store.books());
    editor.update(Message::Text(Field::Given, "Grace".into()));
    editor.update(Message::Text(Field::Family, "Hopper".into()));
    let grace = editor.finish();
    fixture.store.save(&grace).expect("save");
    assert_eq!(fixture.store.contacts().len(), 2);

    fixture
        .store
        .delete(&fixture.book.id, &grace.uid)
        .expect("delete");

    let remaining = fixture.store.contacts();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].uid, "ada@server");
    assert!(fixture.on_disk().contains("PHOTO;ENCODING=b:AAAABBBBCCCC"));
}

/// A read-only book must refuse a save rather than failing silently or, worse,
/// appearing to succeed.
#[test]
fn saving_into_a_read_only_book_is_an_error() {
    let fixture = fixture();

    let mut meta = fixture.book.clone();
    meta.read_only = true;
    let result = cosmic_pim_core::store::contacts::write_contact(&meta, &fixture.ada());

    assert!(result.is_err(), "a read-only book accepted a write");
    assert!(
        fixture.on_disk().contains("FN:Ada Lovelace"),
        "the card changed anyway"
    );
}
