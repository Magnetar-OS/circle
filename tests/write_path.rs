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

use circle::ui::editor::{Field, GroupRow, ListKind, Message, State};
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

/// The version policy, end to end: new cards default to 3.0 with its own
/// preference spelling; 4.0 is the explicit choice; an existing card keeps its
/// version through an edit.
#[test]
fn new_cards_default_to_v3_and_the_choice_is_explicit() {
    use cosmic_pim_core::vcard::WriteVersion;

    let mut fixture = fixture();

    let mut editor = State::create(&fixture.book.id, fixture.store.books());
    editor.update(Message::Text(Field::Given, "Grace".into()));
    editor.update(Message::ListAdd(ListKind::Email));
    editor.update(Message::ListValue(
        ListKind::Email,
        0,
        "g@example.com".into(),
    ));
    editor.update(Message::ListPreferred(ListKind::Email, 0));
    let grace = editor.finish();

    // The default path — what Circle's save uses with the toggle off.
    fixture.store.save(&grace).unwrap();
    let on_disk = std::fs::read_to_string(
        fixture.book.path.join(
            &fixture
                .store
                .contact(&fixture.book.id, &grace.uid)
                .unwrap()
                .file_name,
        ),
    )
    .unwrap();
    assert!(on_disk.contains("VERSION:3.0"), "{on_disk}");
    assert!(
        on_disk.contains("TYPE=pref") || on_disk.contains("TYPE=home,pref"),
        "{on_disk}"
    );
    assert!(
        !on_disk.contains("PREF="),
        "4.0 syntax in a 3.0 card: {on_disk}"
    );

    // The explicit choice.
    let mut editor = State::create(&fixture.book.id, fixture.store.books());
    editor.update(Message::Text(Field::Given, "Alan".into()));
    let alan = editor.finish();
    fixture.store.save_as(&alan, WriteVersion::V4).unwrap();
    let alan_file = fixture
        .store
        .contact(&fixture.book.id, &alan.uid)
        .unwrap()
        .file_name;
    let on_disk = std::fs::read_to_string(fixture.book.path.join(alan_file)).unwrap();
    assert!(on_disk.contains("VERSION:4.0"), "{on_disk}");

    // Editing the synced 4.0 seed card must not downgrade it.
    let mut editor = fixture.editor();
    editor.update(Message::Text(Field::Family, "Byron".into()));
    fixture.store.save(&editor.finish()).unwrap();
    assert!(
        fixture.on_disk().contains("VERSION:4.0"),
        "an edit converted the card's version"
    );
}

/// The photo write path the shell drives on save: read the saved bytes, patch
/// the photo in (or out), write raw. Everything else on the card survives.
#[test]
fn setting_and_removing_a_photo_preserves_the_rest_of_the_card() {
    use cosmic_pim_core::store::contacts::write_contact_raw;
    use cosmic_pim_core::vcard::{Photo, photo, remove_photo, set_photo};

    let fixture = fixture();
    let png = [0x89u8, b'P', b'N', b'G', 0x0d, 0x0a, 0x1a, 0x0a];

    // Set: replaces the seed card's PHOTO, keeps its grouped label and GEO.
    let saved = fixture.ada();
    let patched = set_photo(&saved.raw, &saved.uid, &png, "image/png").expect("patch");
    write_contact_raw(&fixture.book, &saved.file_name, &patched).expect("write");

    let back = fixture.ada();
    assert!(back.has_photo);
    match photo(&back.raw) {
        Some(Photo::Bytes { data, .. }) => assert_eq!(data, png),
        other => panic!("photo did not round trip: {other:?}"),
    }
    assert!(
        back.raw.contains("item1.X-ABLabel:Summer house"),
        "{}",
        back.raw
    );
    assert!(back.raw.contains("GEO:"), "{}", back.raw);

    // Remove: the photo goes, nothing else does.
    let stripped = remove_photo(&back.raw, &back.uid).expect("patch");
    write_contact_raw(&fixture.book, &back.file_name, &stripped).expect("write");
    let back = fixture.ada();
    assert!(!back.has_photo);
    assert!(
        back.raw.contains("item1.X-ABLabel:Summer house"),
        "{}",
        back.raw
    );
    assert_eq!(back.emails.len(), 2);
}

/// Group membership, end to end through the editor's rows and the store: the
/// change lands on the GROUP card in its own spelling, and only the changed
/// group is touched.
#[test]
fn toggling_membership_patches_the_group_card_and_only_it() {
    use cosmic_pim_core::vcard::{WriteVersion, member_uid, member_uri};

    let mut fixture = fixture();
    let friends = fixture
        .store
        .create_group("Friends", &fixture.book.id, WriteVersion::V3)
        .unwrap();
    let work = fixture
        .store
        .create_group("Work", &fixture.book.id, WriteVersion::V3)
        .unwrap();
    let work_before = fixture
        .store
        .contact(&fixture.book.id, &work.uid)
        .unwrap()
        .raw;

    // The editor flow: rows built from the store, one toggled, save applies
    // the diff (mirroring AppModel::save_editor's steps without the shell).
    let contact = fixture.ada();
    let mut editor = State::edit(contact.clone(), fixture.store.books()).with_groups(vec![
        GroupRow {
            uid: friends.uid.clone(),
            name: "Friends".into(),
            member: false,
            was_member: false,
        },
        GroupRow {
            uid: work.uid.clone(),
            name: "Work".into(),
            member: false,
            was_member: false,
        },
    ]);
    editor.update(Message::GroupToggled(0, true));
    assert_eq!(editor.changed_groups().len(), 1, "only Friends changed");

    let saved = editor.finish();
    fixture.store.save(&saved).unwrap();
    for row in editor.changed_groups() {
        let group = fixture.store.contact(&fixture.book.id, &row.uid).unwrap();
        let mut members = group.members.clone();
        members.push(member_uri(&saved.uid));
        fixture
            .store
            .set_group_members(&fixture.book.id, &row.uid, &members)
            .unwrap();
    }

    // Membership landed, in the Apple spelling the 3.0 group card uses.
    let friends_after = fixture
        .store
        .contact(&fixture.book.id, &friends.uid)
        .unwrap();
    assert!(
        friends_after
            .members
            .iter()
            .any(|uri| member_uid(uri) == Some(saved.uid.as_str())),
        "{:?}",
        friends_after.members
    );
    assert!(
        friends_after.raw.contains("X-ADDRESSBOOKSERVER-MEMBER:"),
        "a 3.0 group gained the wrong member spelling:\n{}",
        friends_after.raw
    );

    // The untouched group's file did not change at all.
    let work_after = fixture
        .store
        .contact(&fixture.book.id, &work.uid)
        .unwrap()
        .raw;
    assert_eq!(work_before, work_after, "an unchanged group was rewritten");

    // And the group cards stay out of the people list.
    assert!(
        fixture.store.contacts().iter().all(|c| !c.is_group),
        "a group card leaked into the contact list"
    );
}

/// CSV import, end to end: parse a real file, map, land in the store — and a
/// re-import keyed on a mapped UID updates through the patcher rather than
/// duplicating.
#[test]
fn csv_import_lands_and_a_uid_keyed_reimport_updates_losslessly() {
    use circle::ui::csv;

    let mut fixture = fixture();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("export.csv");
    std::fs::write(
        &path,
        "First Name,Last Name,E-mail 1,UID\n\
Grace,Hopper,grace@example.com,grace@import\n\
\"Lovelace, Ada\",,ada2@example.com,ada2@import\n",
    )
    .unwrap();

    let state = csv::State::open(&path).unwrap();
    // Headers here pre-map exactly; a real user could remap in the UI.
    let (contacts, skipped) = state.contacts(&fixture.book.id);
    assert_eq!(skipped, 0);
    for contact in &contacts {
        fixture.store.save(contact).unwrap();
    }
    assert_eq!(
        fixture.store.contacts().len(),
        3,
        "2 imported beside the seed card"
    );

    // Simulate a photo landing on the imported card from a sync…
    let grace = fixture
        .store
        .contact(&fixture.book.id, "grace@import")
        .unwrap();
    let with_photo =
        cosmic_pim_core::vcard::set_photo(&grace.raw, &grace.uid, &[1, 2, 3], "image/png").unwrap();
    cosmic_pim_core::store::contacts::write_contact_raw(
        &fixture.book,
        &grace.file_name,
        &with_photo,
    )
    .unwrap();

    // …then re-import the same file, adopting the existing card the way the
    // shell does. The photo must survive the update.
    let state = csv::State::open(&path).unwrap();
    let (contacts, _) = state.contacts(&fixture.book.id);
    for mut contact in contacts {
        if let Some(existing) = fixture.store.contact(&fixture.book.id, &contact.uid) {
            contact.file_name = existing.file_name;
            contact.raw = existing.raw;
        }
        fixture.store.save(&contact).unwrap();
    }

    assert_eq!(
        fixture.store.contacts().len(),
        3,
        "the re-import duplicated instead of updating"
    );
    let grace = fixture
        .store
        .contact(&fixture.book.id, "grace@import")
        .unwrap();
    assert!(
        grace.has_photo,
        "the CSV re-import destroyed the photo a sync had added"
    );
}

/// The auto-merge contract, from Circle's side: a save queued after an edit
/// carries the pre-edit bytes as its base — and a second edit before the push
/// drains does NOT move it, because the server still holds the original.
///
/// The base is what lets the sync engine three-way-merge (`merge::overlaps`)
/// instead of raising a conflict when the server changed the same card; a
/// Circle that queued without it would silently disable that for every edit.
#[test]
fn a_queued_edit_carries_its_pre_edit_base_and_the_first_base_sticks() {
    use cosmic_pim_caldav::VdirStore;
    use cosmic_pim_caldav::push::PushQueue as _;

    let mut fixture = fixture();

    // Bind the book to a (pretend) CardDAV collection, the way a provisioned
    // account would.
    {
        let meta = fixture.book.clone();
        let mut vstore = VdirStore::open(meta).expect("open vdir store");
        vstore.set_remote("/dav/contacts/", false).expect("bind");
    }

    // First edit: what the editor does — read, patch, save, queue with the
    // text it read.
    let before_first = fixture.ada().raw;
    let mut editor = fixture.editor();
    editor.update(Message::Text(Field::Family, "Byron".into()));
    let saved = editor.finish();
    fixture.store.save(&saved).unwrap();
    cosmic_pim_sync::queue_save_with_base(
        fixture.store.root(),
        &fixture.book.id,
        &saved.file_name,
        Some(&before_first),
    )
    .expect("queue");

    // Second edit before any push drains.
    let before_second = fixture.ada().raw;
    assert_ne!(before_first, before_second, "the first edit did not land");
    let mut editor = fixture.editor();
    editor.update(Message::Text(Field::Given, "Augusta".into()));
    let saved = editor.finish();
    fixture.store.save(&saved).unwrap();
    cosmic_pim_sync::queue_save_with_base(
        fixture.store.root(),
        &fixture.book.id,
        &saved.file_name,
        Some(&before_second),
    )
    .expect("queue");

    let pending = VdirStore::open(fixture.book.clone())
        .expect("reopen")
        .pending();
    assert_eq!(pending.len(), 1, "one entry per href, reset not appended");
    assert_eq!(
        pending[0].base.as_deref(),
        Some(before_first.as_str()),
        "the base moved on re-enqueue — the server still holds the FIRST text, \
         and merging against the second would silently drop the first edit"
    );
}

/// A parameter the model does not name, on a property the model rewrites.
///
/// Every other fixture here tests unmodelled *properties* — PHOTO, GEO,
/// `X-ABShowAs` — which survive because the patcher never touches their
/// lines. A parameter is a different question: it rides on a line the patcher
/// does rewrite, so preserving it means keeping part of a line while
/// replacing the rest.
///
/// Real cards carry these. `EMAIL;TYPE=work;X-SERVICE=slack:` is what a
/// contact synced from a client with service integrations looks like. The
/// question was worth asking here because a sibling application lost exactly
/// this shape — `SUMMARY;LANGUAGE=en-gb` exporting as plain `SUMMARY` — to a
/// re-serialising path, and Circle's whole promise is that it patches instead.
const PARAMETERISED: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@params\r\n\
FN:Ada Lovelace\r\n\
N:Lovelace;Ada;;;\r\n\
EMAIL;TYPE=work;X-SERVICE=slack:ada@work.example\r\n\
TEL;TYPE=cell;X-CARRIER=cosmote:+30 694 1234567\r\n\
END:VCARD\r\n";

/// Closed by `Typed::params`, which carries a line's other parameters *in the
/// entry* rather than reconstructing them at write time.
#[test]
fn parameters_on_untouched_lines_survive_an_edit() {
    use cosmic_pim_core::vcard::{parse_vcards, patch_vcard};

    let mut contact = parse_vcards(PARAMETERISED, "book", "ada.vcf").remove(0);
    // Edit a field nowhere near the parameterised lines.
    contact.display_name = "Ada, Countess of Lovelace".into();

    let patched = patch_vcard(&contact.raw, &contact).expect("the card patches");

    assert!(
        patched.contains("Countess"),
        "the edit did not land: {patched}"
    );
    assert!(
        patched.contains("X-SERVICE=slack"),
        "editing a name dropped a parameter from an untouched EMAIL line: {patched}"
    );
    assert!(
        patched.contains("X-CARRIER=cosmote"),
        "editing a name dropped a parameter from an untouched TEL line: {patched}"
    );
}

/// The harder half: the parameter sits on a line whose **value** is being
/// rewritten, so it cannot be preserved by leaving the line alone.
/// The half that made the design: a parameter here cannot be preserved by
/// leaving the line alone. Carrying it in the entry dissolves the problem —
/// the `Typed` the interface edited is the one that holds them, so there is
/// no write-time join between old lines and new values, and therefore no
/// positional matching to get wrong.
#[test]
fn parameters_survive_a_rewrite_of_the_value_they_sit_on() {
    use cosmic_pim_core::vcard::{parse_vcards, patch_vcard};

    let mut contact = parse_vcards(PARAMETERISED, "book", "ada.vcf").remove(0);
    contact.emails[0].value = "ada@newwork.example".into();

    let patched = patch_vcard(&contact.raw, &contact).expect("the card patches");

    assert!(
        patched.contains("ada@newwork.example"),
        "the edit did not land: {patched}"
    );
    assert!(
        patched.contains("X-SERVICE=slack"),
        "changing an address dropped the service parameter beside it: {patched}"
    );
}

/// The join the substrate's own tests cannot make: parameters have to survive
/// a round trip through *this* application's editor, not only through the
/// patcher.
///
/// Carrying them in `Typed` is only half the fix. An interface that rebuilt
/// its entries from its own field state — which is a perfectly ordinary way
/// to write an editor — would hand the patcher entries with empty `params`
/// and drop them just as thoroughly, while every test in the substrate
/// carried on passing. Circle mutates the entries it was given in place, and
/// this is the test that says so.
#[test]
fn parameters_survive_a_round_trip_through_the_editor() {
    use cosmic_pim_core::store::contacts::write_contact_raw;

    let mut fixture = fixture();
    write_contact_raw(&fixture.book, "params.vcf", PARAMETERISED).expect("seed");
    fixture.store.refresh();

    let contact = fixture
        .store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@params")
        .expect("the seeded contact");

    // Edit through the real editor: rename, retype an existing entry, and add
    // a new one — the three things that touch a `Typed` in different ways.
    let mut editor = State::edit(contact, fixture.store.books());
    editor.update(Message::Text(Field::DisplayName, "Ada Byron".into()));
    editor.update(Message::ListValue(
        ListKind::Email,
        0,
        "ada@newwork.example".into(),
    ));
    editor.update(Message::ListAdd(ListKind::Phone));
    let added = editor.contact.phones.len() - 1;
    editor.update(Message::ListValue(
        ListKind::Phone,
        added,
        "+30 210 5555555".into(),
    ));

    fixture.store.save(&editor.finish()).expect("save");

    let card = std::fs::read_to_string(fixture.book.path.join("params.vcf")).expect("read it back");

    assert!(
        card.contains("Ada Byron"),
        "the rename did not land: {card}"
    );
    assert!(
        card.contains("ada@newwork.example"),
        "the address edit did not land: {card}"
    );
    assert!(
        card.contains("X-SERVICE=slack"),
        "the editor dropped the parameter on the line it edited: {card}"
    );
    assert!(
        card.contains("X-CARRIER=cosmote"),
        "the editor dropped the parameter on a line it did not touch: {card}"
    );
    // The added entry came from no line, so it carries nothing — and must not
    // have inherited another entry's parameters.
    let added_line = card
        .lines()
        .find(|line| line.contains("+30 210 5555555"))
        .expect("the added number is on the card");
    assert!(
        !added_line.contains("X-CARRIER"),
        "a freshly added entry inherited another line's parameters: {added_line}"
    );
}

/// One level below a line: a structured **value**.
///
/// A line holds more than its value; a value holds more than one component.
/// `N` has five and `ADR` has seven, and the model names all of them. `ORG`
/// is a hierarchy — `company;department;team` — and `Contact::organisation`
/// is one `String`, so this asks whether the parts the model does not name
/// come back.
const STRUCTURED: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@structured\r\n\
FN:Ada Lovelace\r\n\
N:Lovelace;Ada;Augusta;Ms.;FRS\r\n\
ORG:Analytical Engine Co;Research;Difference Engines\r\n\
ADR;TYPE=work:PO Box 12;Suite 4;12 Marylebone Rd;London;Greater London;NW1 5LA;UK\r\n\
END:VCARD\r\n";

/// A card patched, then unfolded so assertions see logical lines.
///
/// Folding at 75 octets is correct and is not what these tests are about; a
/// naive `contains` on the folded text fails on a postcode split across a
/// continuation, which says nothing about preservation.
fn patched_unfolded(card: &str, edit: impl FnOnce(&mut cosmic_pim_core::model::Contact)) -> String {
    use cosmic_pim_core::vcard::{parse_vcards, patch_vcard};

    let mut contact = parse_vcards(card, "book", "ada.vcf").remove(0);
    edit(&mut contact);
    patch_vcard(&contact.raw, &contact)
        .expect("the card patches")
        .replace("\r\n ", "")
}

#[test]
fn n_and_adr_keep_every_component_through_an_edit() {
    let patched = patched_unfolded(STRUCTURED, |c| c.display_name = "Ada Byron".into());
    assert!(
        patched.contains("Ada Byron"),
        "the edit did not land: {patched}"
    );

    // N has five components and ADR has seven; the model names all of them,
    // so both survive a patch whole.
    assert!(
        patched.contains("N:Lovelace;Ada;Augusta;Ms.;FRS"),
        "a component of N was lost: {patched}"
    );
    assert!(
        patched.contains(
            "ADR;TYPE=work:PO Box 12;Suite 4;12 Marylebone Rd;London;\
                          Greater London;NW1 5LA;UK"
        ),
        "a component of ADR was lost: {patched}"
    );
}

/// Closed by `Contact::organisation_units`, which carries the department
/// levels beside the company name rather than flattening them into it — kept
/// separate because the components are escaped individually, and a join would
/// write one name that happens to contain semicolons, which is a different
/// fact about the contact.
#[test]
fn org_keeps_its_department_levels_through_an_edit() {
    let patched = patched_unfolded(STRUCTURED, |c| c.display_name = "Ada Byron".into());
    assert!(
        patched.contains("ORG:Analytical Engine Co;Research;Difference Engines"),
        "the organisation's department levels were lost: {patched}"
    );
}

/// A category containing a comma, through the editor.
///
/// The substrate now unescapes `CATEGORIES:work,friends\, close,vip` into
/// three values, one of which contains a comma. The editor shows categories
/// as comma-separated text and splits that text on save — so a value with a
/// comma in it becomes two, and the escape is destroyed on the next write.
///
/// This is the obligation the substrate documents but cannot enforce, in its
/// second form: not rebuilding an *entry* from field state, but rebuilding a
/// *list* from a flattened rendering of it. Preservation below the model is
/// only real if the interface preserves it too.
const CATEGORISED: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@cats\r\n\
FN:Ada Lovelace\r\n\
CATEGORIES:work,friends\\, close,vip\r\n\
END:VCARD\r\n";

#[test]
fn a_category_containing_a_comma_survives_the_editor() {
    use cosmic_pim_core::store::contacts::write_contact_raw;

    let mut fixture = fixture();
    write_contact_raw(&fixture.book, "cats.vcf", CATEGORISED).expect("seed");
    fixture.store.refresh();

    let contact = fixture
        .store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@cats")
        .expect("the seeded contact");
    assert_eq!(
        contact.categories,
        vec!["work", "friends, close", "vip"],
        "the substrate stopped unescaping, so this tests the wrong thing"
    );

    // Edit something else entirely and save through the real editor.
    let mut editor = State::edit(contact, fixture.store.books());
    editor.update(Message::Text(Field::DisplayName, "Ada Byron".into()));
    fixture.store.save(&editor.finish()).expect("save");

    let back = fixture
        .store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@cats")
        .expect("still there");

    assert_eq!(
        back.categories,
        vec!["work", "friends, close", "vip"],
        "the editor split a category on the comma inside it"
    );
}

/// A birthday with no year.
///
/// `BDAY:--0415` is legal vCard and common from people who would rather not
/// state an age. The substrate models it separately from a full date —
/// `birthday_month_day` beside `birthday` — because one property maps to two
/// fields, and the editor offers a single date box for the pair.
///
/// So this asks both halves of the question the ladder ends on: does the
/// narrow projection *lose* it, and does the interface *show* it.
const AGELESS: &str = "BEGIN:VCARD\r\n\
VERSION:4.0\r\n\
UID:ada@ageless\r\n\
FN:Ada Lovelace\r\n\
BDAY:--0415\r\n\
END:VCARD\r\n";

/// **Known gap, not yet fixed — in the substrate.** `patch_vcard` writes
/// `BDAY` from `contact.birthday.iter()`, which yields nothing for a year-less
/// date, so `set` receives an empty line list and that means *remove the
/// property*. `birthday_month_day` is never consulted, and the line is gone.
///
/// A new shape rather than a new rung: one property maps to two model fields,
/// and the writer knows only one of them. Reported with this reproduction;
/// kept ignored so `cargo test` names it on every run, and written as the
/// acceptance test.
///
/// Circle's own half — showing a year-less birthday rather than leaving it
/// invisible — is fixed, and covered in `ui::person`.
#[ignore = "known gap in the substrate: patch_vcard writes BDAY from `birthday` only, so a year-less one is removed"]
#[test]
fn a_birthday_with_no_year_survives_an_edit() {
    use cosmic_pim_core::store::contacts::write_contact_raw;

    let mut fixture = fixture();
    write_contact_raw(&fixture.book, "ageless.vcf", AGELESS).expect("seed");
    fixture.store.refresh();

    let contact = fixture
        .store
        .contacts()
        .into_iter()
        .find(|c| c.uid == "ada@ageless")
        .expect("the seeded contact");
    assert_eq!(
        contact.birthday_month_day,
        Some((4, 15)),
        "the substrate stopped modelling a year-less BDAY, so this tests the wrong thing"
    );
    assert!(
        contact.birthday.is_none(),
        "a year-less BDAY is not a full date"
    );

    // Edit something else entirely, through the real editor.
    let mut editor = State::edit(contact, fixture.store.books());
    editor.update(Message::Text(Field::DisplayName, "Ada Byron".into()));
    fixture.store.save(&editor.finish()).expect("save");

    let card =
        std::fs::read_to_string(fixture.book.path.join("ageless.vcf")).expect("read it back");
    assert!(card.contains("Ada Byron"), "the edit did not land: {card}");
    assert!(
        card.contains("BDAY:--0415"),
        "editing a name destroyed a birthday that had no year: {card}"
    );
}
