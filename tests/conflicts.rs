// SPDX-License-Identifier: GPL-3.0-only

//! Sync conflicts, reached the way a real one is: a synced card edited here,
//! then changed on the server before the edit was pushed.

use circle::conflicts::{self, Resolution, ResolveError};
use cosmic_pim_caldav::push::PushQueue as _;
use cosmic_pim_caldav::{CalDavStore as _, Conflict, ConflictKind, RemoteEvent, VdirStore};
use cosmic_pim_core::merge::Side;
use cosmic_pim_core::model::Rgb;
use cosmic_pim_core::store::vdir;
use std::path::Path;

const HREF: &str = "/dav/ab/ada.vcf";
const BASE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\n\
TEL:+44 20 1111\r\nEMAIL:ada@example.com\r\nEND:VCARD\r\n";

/// A bound address book holding one card in conflict: `local` is this
/// device's unsent edit of [`BASE`], `remote` the server's.
fn conflicted(local: &str, remote: &str) -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let meta = vdir::create_collection(dir.path(), "Personal", Rgb(1, 2, 3)).unwrap();
    let id = meta.id.clone();

    let mut store = VdirStore::open_carddav(meta).unwrap();
    store.set_remote("/dav/ab/", false).unwrap();
    store
        .upsert(&RemoteEvent {
            href: HREF.into(),
            etag: "\"v1\"".into(),
            ics: BASE.into(),
        })
        .unwrap();
    std::fs::write(store.collection().path.join("ada.vcf"), local).unwrap();
    store.queue_put_with_base(HREF, Some(BASE)).unwrap();
    store
        .record_conflict(&Conflict {
            href: HREF.into(),
            kind: ConflictKind::BothEdited,
            local: local.into(),
            remote: remote.into(),
            remote_etag: "\"v2\"".into(),
            base: Some(BASE.into()),
        })
        .unwrap();
    (dir, id)
}

fn card(root: &Path, id: &str) -> String {
    std::fs::read_to_string(root.join(id).join("ada.vcf")).unwrap()
}

fn push_is_live(root: &Path, id: &str) -> bool {
    let meta = vdir::collections(root)
        .into_iter()
        .find(|m| m.id == id)
        .unwrap();
    let pending = VdirStore::open(meta).unwrap().pending().unwrap();
    pending.len() == 1 && !pending[0].blocked
}

#[test]
fn a_conflict_is_listed_with_both_names_and_its_book() {
    let local = BASE.replace("FN:Ada Lovelace", "FN:Ada King");
    let remote = BASE.replace("FN:Ada Lovelace", "FN:Augusta Ada King");
    let (dir, _) = conflicted(&local, &remote);

    let rows = conflicts::load(dir.path());
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].book_name, "Personal");
    assert_eq!(rows[0].yours, "Ada King");
    assert_eq!(rows[0].theirs, "Augusta Ada King");
}

#[test]
fn taking_the_servers_card_replaces_mine_and_clears_the_conflict() {
    let local = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 2222");
    let remote = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 3333");
    let (dir, id) = conflicted(&local, &remote);
    let row = conflicts::load(dir.path()).remove(0);

    conflicts::resolve(dir.path(), &row, Resolution::TakeTheirs).unwrap();

    assert_eq!(card(dir.path(), &id), remote);
    assert!(conflicts::load(dir.path()).is_empty());
}

#[test]
fn keeping_mine_re_queues_it_for_upload() {
    let local = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 2222");
    let remote = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 3333");
    let (dir, id) = conflicted(&local, &remote);
    let row = conflicts::load(dir.path()).remove(0);

    conflicts::resolve(dir.path(), &row, Resolution::KeepMine).unwrap();

    assert_eq!(card(dir.path(), &id), local);
    assert!(push_is_live(dir.path(), &id), "the push stayed parked");
    assert!(conflicts::load(dir.path()).is_empty());
}

/// The server changed the email, this device the number: nothing to ask.
#[test]
fn edits_to_different_properties_merge_without_a_question() {
    let local = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 2222");
    let remote = BASE.replace("EMAIL:ada@example.com", "EMAIL:ada@analytical.org");
    let (dir, id) = conflicted(&local, &remote);
    let row = conflicts::load(dir.path()).remove(0);
    let disputes = row.disputes.as_ref().expect("the base was recorded");
    assert!(disputes.units.is_empty());

    conflicts::resolve(dir.path(), &row, Resolution::Merge).unwrap();

    let merged = card(dir.path(), &id);
    assert!(merged.contains("TEL:+44 20 2222"), "{merged}");
    assert!(merged.contains("EMAIL:ada@analytical.org"), "{merged}");
    assert!(push_is_live(dir.path(), &id));
}

/// Both sides renamed her: the name is asked about, everything else merges.
#[test]
fn a_disputed_property_is_merged_from_the_side_chosen() {
    let local = BASE
        .replace("FN:Ada Lovelace", "FN:Ada King")
        .replace("TEL:+44 20 1111", "TEL:+44 20 2222");
    let remote = BASE
        .replace("FN:Ada Lovelace", "FN:Augusta Ada King")
        .replace("EMAIL:ada@example.com", "EMAIL:ada@analytical.org");
    let (dir, id) = conflicted(&local, &remote);
    let mut row = conflicts::load(dir.path()).remove(0);

    assert_eq!(
        conflicts::resolve(dir.path(), &row, Resolution::Merge),
        Err(ResolveError::MergeFailed),
        "an undecided merge was written"
    );

    let disputes = row.disputes.as_mut().unwrap();
    assert_eq!(disputes.units.len(), 1);
    assert_eq!(disputes.units[0].unit, "FN");
    disputes.choose(0, Side::Local);
    conflicts::resolve(dir.path(), &row, Resolution::Merge).unwrap();

    let merged = card(dir.path(), &id);
    assert!(merged.contains("FN:Ada King\r\n"), "{merged}");
    assert!(merged.contains("TEL:+44 20 2222"), "{merged}");
    assert!(merged.contains("EMAIL:ada@analytical.org"), "{merged}");
    assert!(conflicts::load(dir.path()).is_empty());
}

/// A synced card with a conflict of the given deletion kind recorded
/// against it, the way the substrate records one.
fn deletion_conflict(kind: ConflictKind) -> (tempfile::TempDir, String) {
    let edited = BASE.replace("TEL:+44 20 1111", "TEL:+44 20 2222");
    let dir = tempfile::tempdir().unwrap();
    let meta = vdir::create_collection(dir.path(), "Personal", Rgb(1, 2, 3)).unwrap();
    let id = meta.id.clone();
    let mut store = VdirStore::open_carddav(meta).unwrap();
    store.set_remote("/dav/ab/", false).unwrap();
    store
        .upsert(&RemoteEvent {
            href: HREF.into(),
            etag: "\"v1\"".into(),
            ics: BASE.into(),
        })
        .unwrap();
    let file = store.collection().path.join("ada.vcf");
    let (local, remote) = match kind {
        ConflictKind::DeletedOnServer => {
            std::fs::write(&file, &edited).unwrap();
            store.queue_put_with_base(HREF, Some(BASE)).unwrap();
            (edited, String::new())
        }
        ConflictKind::DeletedHere => {
            std::fs::remove_file(&file).unwrap();
            store.queue_delete(HREF).unwrap();
            (String::new(), edited)
        }
        ConflictKind::BothEdited => unreachable!("see conflicted()"),
    };
    store
        .record_conflict(&Conflict {
            href: HREF.into(),
            kind,
            local,
            remote,
            remote_etag: "\"v2\"".into(),
            base: Some(BASE.into()),
        })
        .unwrap();
    (dir, id)
}

/// Each kind is worded for what its buttons do; the same "Keep mine" on a
/// card deleted here would delete it on the server without saying so.
#[test]
fn each_kind_of_conflict_is_worded_for_what_it_does() {
    let local = BASE.replace("FN:Ada Lovelace", "FN:Ada King");
    let (both, _) = conflicted(&local, BASE);
    let (on_server, _) = deletion_conflict(ConflictKind::DeletedOnServer);
    let (here, _) = deletion_conflict(ConflictKind::DeletedHere);

    let words: Vec<conflicts::Wording> = [&both, &on_server, &here]
        .iter()
        .map(|dir| conflicts::wording(&conflicts::load(dir.path())[0]))
        .collect();

    for (i, a) in words.iter().enumerate() {
        for b in &words[i + 1..] {
            assert_ne!(a.summary, b.summary);
            assert_ne!(a.keep_mine, b.keep_mine);
            assert_ne!(a.take_theirs, b.take_theirs);
        }
    }
    // A deletion names the card from the side that still has it.
    assert!(
        words[1].summary.contains("Ada Lovelace"),
        "{}",
        words[1].summary
    );
    assert!(
        words[2].summary.contains("Ada Lovelace"),
        "{}",
        words[2].summary
    );
}

/// A deletion on one side leaves nothing to merge property by property.
#[test]
fn a_deletion_conflict_offers_no_merge() {
    let (dir, _) = deletion_conflict(ConflictKind::DeletedOnServer);
    assert!(conflicts::load(dir.path())[0].disputes.is_none());
}

/// "Delete here too" on a card the server deleted removes it here.
#[test]
fn taking_the_servers_deletion_deletes_the_card_here() {
    let (dir, id) = deletion_conflict(ConflictKind::DeletedOnServer);
    let row = conflicts::load(dir.path()).remove(0);

    conflicts::resolve(dir.path(), &row, Resolution::TakeTheirs).unwrap();

    assert!(!dir.path().join(&id).join("ada.vcf").exists());
    assert!(conflicts::load(dir.path()).is_empty());
}

/// "Restore server's" on a card deleted here brings the server's copy back.
#[test]
fn restoring_the_servers_copy_undoes_the_local_deletion() {
    let (dir, id) = deletion_conflict(ConflictKind::DeletedHere);
    let row = conflicts::load(dir.path()).remove(0);

    conflicts::resolve(dir.path(), &row, Resolution::TakeTheirs).unwrap();

    assert!(card(dir.path(), &id).contains("TEL:+44 20 2222"));
    assert!(conflicts::load(dir.path()).is_empty());
}
