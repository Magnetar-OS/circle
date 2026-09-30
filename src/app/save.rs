// SPDX-License-Identifier: GPL-3.0-only

//! Committing the editor: the card, its photo, its group memberships,
//! and the bulk twin of the categories field.

use cosmic::app::Task;
use cosmic_pim_core::model::Contact;
use cosmic_pim_core::store::StoreError;
use cosmic_pim_core::store::contacts::ContactStore;

use super::sync::queue_push_with_base;
use super::{AppModel, ContactKey, Message};
use crate::fl;
use crate::ui::dialogs::Dialog;
use crate::ui::editor;

impl AppModel {
    /// Adds every checked contact to the named `CATEGORIES` group — the
    /// bulk twin of the editor's categories field.
    pub(super) fn add_checked_to_group(&mut self) -> Task<Message> {
        let Some(Dialog::AddToGroup { name }) = self.dialog.take() else {
            return Task::none();
        };
        let name = name.trim().to_owned();
        if name.is_empty() {
            return Task::none();
        }
        let version = self.write_version();
        let keys: Vec<ContactKey> = self.checked.iter().cloned().collect();
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        let mut joined = 0usize;
        let mut first_error: Option<String> = None;
        for key in keys {
            let Some(mut contact) = store.contact(&key.book, &key.uid) else {
                continue;
            };
            if contact.categories.iter().any(|c| c == &name) {
                continue;
            }
            let base = contact.raw.clone();
            contact.categories.push(name.clone());
            match store.save_as(&contact, version) {
                Ok(()) => {
                    // The card was read two lines up; its raw is the pre-edit
                    // text sync can merge against.
                    if let Err(why) = queue_push_with_base(
                        store,
                        &key.book,
                        &contact.file_name,
                        (!base.trim().is_empty()).then_some(base.as_str()),
                    ) {
                        first_error.get_or_insert(why);
                    }
                    joined += 1;
                }
                Err(why) => {
                    first_error.get_or_insert_with(|| save_error(&contact.label(), &why));
                }
            }
        }

        self.selecting = false;
        self.checked.clear();
        self.rebuild_nav();
        self.reload();
        if let Some(why) = first_error {
            return self.toast(why);
        }
        self.toast(fl!(
            "added-to-group",
            count = joined.to_string(),
            name = name
        ))
    }

    /// Commits the editor to the store.
    pub(super) fn save_editor(&mut self) -> Task<Message> {
        let Some(state) = self.editor.as_ref() else {
            return Task::none();
        };
        if !state.is_saveable() {
            return Task::none();
        }

        let contact = state.finish();
        let photo_edit = state.photo.clone();
        let removed_groups = state.removed_groups();
        let changed_groups: Vec<editor::GroupRow> =
            state.changed_groups().into_iter().cloned().collect();
        let version = self.write_version();
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        // The version applies to NEW cards only; an existing card keeps the
        // version its bytes declare, because saving patches rather than
        // converts.
        if let Err(why) = store.save_as(&contact, version) {
            // Deliberately keeps the editor open: the save failed, so the
            // user's text is the only copy that exists. When the card changed
            // on disk underneath the editor, the list is re-read so it shows
            // the other version, and the message says where this one went.
            if matches!(why, StoreError::Conflict { .. }) {
                store.refresh();
                self.photos.remove(&ContactKey::of(&contact));
                self.reload();
            }
            return self.toast(save_error(&contact.label(), &why));
        }

        // The save landed — queue it for upload before anything later in this
        // function can fail. The photo patch below rewrites the same file, so
        // one queue entry covers both. The base is the text the editor was
        // opened on — exactly what this edit was made against — which is what
        // lets sync auto-merge if the server changed the card meanwhile; a
        // brand-new contact has no before and queues without one.
        let queued = match store.contact(&contact.addressbook_id, &contact.uid) {
            Some(saved) => {
                let base = (!contact.raw.trim().is_empty()).then_some(contact.raw.as_str());
                queue_push_with_base(store, &contact.addressbook_id, &saved.file_name, base)
            }
            None => Ok(()),
        };

        // A grouped entry removed in the editor is still on the saved card:
        // the patcher edits grouped lines in place and never removes one. It
        // goes here, with its label. Reported with the queue's outcome — the
        // save itself landed either way.
        let removal = remove_grouped_entries(store, &contact, &removed_groups);
        let queued = queued.and(removal);

        // The photo change runs against the *saved* bytes, which is what makes
        // it uniform for new and existing cards: after the save above, both
        // have a card on disk to patch. The photo is not part of the model on
        // purpose — see `Contact::has_photo` — so it cannot travel through
        // `save_as`.
        if let Err(why) = apply_photo_edit(store, &contact, &photo_edit) {
            self.photos.remove(&ContactKey::of(&contact));
            self.editor = None;
            self.selected = Some(ContactKey::of(&contact));
            self.reload();
            let photo = self.toast(fl!("error-photo", why = why));
            return match queued {
                Err(why) => Task::batch([photo, self.toast(why)]),
                Ok(()) => photo,
            };
        }

        // Membership lives on the GROUP cards, so the changed rows patch those
        // — only the changed ones, or every contact save would churn every
        // group file and push them all to the server unchanged.
        let membership_error =
            apply_group_changes(store, &contact, &changed_groups).or(queued.err());

        // The card's bytes just changed; a cached photo decoded from the old
        // bytes must not survive the save.
        self.photos.remove(&ContactKey::of(&contact));
        self.editor = None;
        self.selected = Some(ContactKey::of(&contact));
        self.rebuild_nav();
        self.reload();
        if let Some(why) = membership_error {
            return self.toast(why);
        }
        Task::none()
    }
}

/// Takes the grouped entries the editor removed out of the just-saved card,
/// each with the label lines that belong to it — see [`crate::grouped`].
fn remove_grouped_entries(
    store: &ContactStore,
    contact: &Contact,
    removed: &[crate::grouped::Entry],
) -> Result<(), String> {
    use cosmic_pim_core::store::contacts::write_contact_raw;

    if removed.is_empty() {
        return Ok(());
    }
    let saved = store
        .contact(&contact.addressbook_id, &contact.uid)
        .ok_or_else(|| fl!("error-load-contacts"))?;
    let stripped = crate::grouped::remove(&saved.raw, &contact.uid, removed)
        .ok_or_else(|| fl!("error-load-contacts"))?;
    let meta = store
        .book(&contact.addressbook_id)
        .ok_or_else(|| fl!("error-load-contacts"))?;
    write_contact_raw(meta, &saved.file_name, &stripped)
        .map_err(|why| save_error(&contact.label(), &why))
}

/// Applies the editor's photo intent to the just-saved card, through the
/// substrate's byte-preserving photo patcher.
///
/// Reads the card back from the store first: only the saved bytes carry the
/// card in its written form (a brand-new contact had no `raw` until now).
fn apply_photo_edit(
    store: &mut ContactStore,
    contact: &Contact,
    edit: &editor::PhotoEdit,
) -> Result<(), String> {
    use cosmic_pim_core::store::contacts::write_contact_raw;
    use cosmic_pim_core::vcard::{remove_photo, set_photo};

    let patched = match edit {
        editor::PhotoEdit::Keep => return Ok(()),
        editor::PhotoEdit::Set(path) => {
            let data = std::fs::read(path).map_err(|why| why.to_string())?;
            let (data, mime) = process_photo(data, photo_mime(path));
            let saved = store
                .contact(&contact.addressbook_id, &contact.uid)
                .ok_or_else(|| fl!("error-load-contacts"))?;
            set_photo(&saved.raw, &contact.uid, &data, mime)
                .ok_or_else(|| fl!("error-load-contacts"))?
        }
        editor::PhotoEdit::Remove => {
            let saved = store
                .contact(&contact.addressbook_id, &contact.uid)
                .ok_or_else(|| fl!("error-load-contacts"))?;
            match remove_photo(&saved.raw, &contact.uid) {
                Some(patched) => patched,
                // No card text to patch means no photo to remove.
                None => return Ok(()),
            }
        }
    };

    let meta = store
        .book(&contact.addressbook_id)
        .ok_or_else(|| fl!("error-load-contacts"))?
        .clone();
    let file_name = store
        .contact(&contact.addressbook_id, &contact.uid)
        .map(|c| c.file_name)
        .ok_or_else(|| fl!("error-load-contacts"))?;
    write_contact_raw(&meta, &file_name, &patched).map_err(|why| why.to_string())
}

/// Applies the editor's membership toggles by patching each changed group
/// card. Returns the first error's message, applying the rest regardless —
/// one unwritable group should not strand the other toggles.
fn apply_group_changes(
    store: &mut ContactStore,
    contact: &Contact,
    changed: &[editor::GroupRow],
) -> Option<String> {
    use cosmic_pim_core::vcard::{member_uid, member_uri};

    let mut first_error = None;
    for row in changed {
        let Some(group) = store.contact(&contact.addressbook_id, &row.uid) else {
            continue; // The group vanished underneath the editor; nothing to do.
        };

        let mut members = group.members.clone();
        if row.member {
            if !members
                .iter()
                .any(|uri| member_uid(uri) == Some(contact.uid.as_str()))
            {
                members.push(member_uri(&contact.uid));
            }
        } else {
            members.retain(|uri| member_uid(uri) != Some(contact.uid.as_str()));
        }

        match store.set_group_members(&contact.addressbook_id, &row.uid, &members) {
            // Queued here rather than by the caller because this is where the
            // group's pre-edit bytes are in hand — the base sync merges
            // against if the server changed the group card meanwhile.
            Ok(()) => {
                if let Err(why) = queue_push_with_base(
                    store,
                    &contact.addressbook_id,
                    &group.file_name,
                    Some(&group.raw),
                ) {
                    first_error.get_or_insert(why);
                }
            }
            Err(why) => {
                first_error.get_or_insert_with(|| why.to_string());
            }
        }
    }
    first_error
}

/// The longest side an embedded photo keeps, in pixels.
///
/// A vCard photo is decoration beside a name, not an archive of the original
/// file — and the original is embedded as base64 into a card that some
/// servers cap at a few megabytes. 512² is larger than any surface Circle
/// draws and small enough that a card stays a card.
const PHOTO_SIDE: u32 = 512;

/// Center-crops a chosen photo square and scales it down to [`PHOTO_SIDE`],
/// re-encoding as JPEG.
///
/// Bytes that already fit — square and small — pass through untouched, so
/// re-setting an exported photo cannot degrade it. Bytes that do not decode
/// at all also pass through: storing what the user picked is strictly better
/// than refusing, and the previous behaviour of this code was exactly that.
fn process_photo(data: Vec<u8>, fallback_mime: &'static str) -> (Vec<u8>, &'static str) {
    let Ok(img) = image::load_from_memory(&data) else {
        tracing::warn!("could not decode the chosen photo; storing it unchanged");
        return (data, fallback_mime);
    };

    let (width, height) = (img.width(), img.height());
    let side = width.min(height);
    if width == height && side <= PHOTO_SIDE {
        return (data, fallback_mime);
    }

    let cropped = img.crop_imm((width - side) / 2, (height - side) / 2, side, side);
    let scaled = if side > PHOTO_SIDE {
        cropped.resize_exact(
            PHOTO_SIDE,
            PHOTO_SIDE,
            image::imageops::FilterType::Lanczos3,
        )
    } else {
        cropped
    };

    // JPEG has no alpha channel, so flatten before encoding.
    let flat = image::DynamicImage::ImageRgb8(scaled.to_rgb8());
    let mut out = Vec::new();
    match flat.write_to(
        &mut std::io::Cursor::new(&mut out),
        image::ImageFormat::Jpeg,
    ) {
        Ok(()) => (out, "image/jpeg"),
        Err(why) => {
            tracing::warn!(%why, "could not re-encode the photo; storing it unchanged");
            (data, fallback_mime)
        }
    }
}

/// Why a card could not be saved, as a sentence for a toast.
///
/// A lost race gets its own words: the file changed on disk between the read
/// and the write (a sync pass, another app), nothing was overwritten, and
/// this version was kept beside it — which the generic message would bury in
/// the substrate's English.
pub(super) fn save_error(name: &str, why: &StoreError) -> String {
    match why {
        StoreError::Conflict { conflict, .. } => fl!(
            "error-save-conflict",
            name = name,
            file = conflict.file_name().map_or_else(
                || conflict.display().to_string(),
                |f| f.to_string_lossy().into_owned()
            )
        ),
        other => fl!("error-save", name = name, why = other.to_string()),
    }
}

/// The MIME type an image file's extension implies. The photo bytes are
/// written as-is; this only labels them.
fn photo_mime(path: &std::path::Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("png") => "image/png",
        Some("webp") => "image/webp",
        Some("gif") => "image/gif",
        _ => "image/jpeg",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The whole path, through a real address book: a grouped address and a
    /// grouped email removed in the editor are gone from the card after the
    /// save, their labels with them, and everything the editor does not
    /// model is still there. Before, the address came back on the next read
    /// and the email could not be removed at all.
    #[test]
    fn a_grouped_entry_removed_in_the_editor_leaves_the_card_with_its_label() {
        use cosmic_pim_core::store::contacts::write_contact_raw;

        const CARD: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\n\
EMAIL;type=WORK:ada@work.example\r\n\
item1.EMAIL;type=INTERNET:ada@home.example\r\nitem1.X-ABLabel:Summer house\r\n\
item2.TEL:+30 210 1234567\r\nitem2.X-ABLabel:Boat\r\n\
item3.ADR;type=HOME:;;1 Main St;Athens;;10431;GR\r\nitem3.X-ABLabel:Winter\r\n\
item3.X-ABADR:gr\r\n\
PHOTO;ENCODING=b:AAAABBBB\r\nX-ABShowAs:COMPANY\r\nEND:VCARD\r\n";

        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let book = store
            .create_book("Home", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        write_contact_raw(&book, "ada.vcf", CARD).unwrap();
        store.refresh();
        let books = store.books().to_vec();

        let mut state = editor::State::edit(store.contact(&book.id, "ada").unwrap(), &books);
        state.update(editor::Message::AddressRemove(0));
        state.update(editor::Message::ListRemove(editor::ListKind::Email, 1));
        let contact = state.finish();
        store.save(&contact).unwrap();
        remove_grouped_entries(&store, &contact, &state.removed_groups()).unwrap();

        let saved = store.contact(&book.id, "ada").unwrap();
        assert!(
            saved.addresses.is_empty(),
            "the address is back: {}",
            saved.raw
        );
        assert_eq!(saved.emails.len(), 1, "{}", saved.raw);
        assert_eq!(saved.emails[0].value, "ada@work.example");
        for orphan in ["item1.", "item3.", "Summer house", "Winter", "X-ABADR"] {
            assert!(
                !saved.raw.contains(orphan),
                "{orphan} was left behind:\n{}",
                saved.raw
            );
        }
        // The grouped phone nobody touched, and what Circle does not model.
        for kept in [
            "item2.TEL:+30 210 1234567\r\nitem2.X-ABLabel:Boat\r\n",
            "PHOTO;ENCODING=b:AAAABBBB\r\n",
            "X-ABShowAs:COMPANY\r\n",
        ] {
            assert!(saved.raw.contains(kept), "{kept} is gone:\n{}", saved.raw);
        }
    }

    /// Nothing removed means nothing written: an ordinary save must not
    /// rewrite the file a second time.
    #[test]
    fn a_save_that_removed_no_grouped_entry_does_not_touch_the_card() {
        let dir = tempfile::tempdir().unwrap();
        let store = ContactStore::open(dir.path()).unwrap();
        // No such book or card: reaching for either would be an error.
        let contact = Contact::draft("nowhere");
        assert_eq!(remove_grouped_entries(&store, &contact, &[]), Ok(()));
    }

    /// A save that lost a race with another writer says so, and names the
    /// file this version was kept in, rather than the substrate's English.
    #[test]
    fn a_save_that_lost_a_race_says_where_the_edit_went() {
        let why = StoreError::Conflict {
            target: "/c/personal/ada.vcf".into(),
            conflict: "/c/personal/ada.vcf.1790000000.conflict".into(),
        };
        let message = save_error("Ada", &why);
        assert_eq!(
            message,
            fl!(
                "error-save-conflict",
                name = "Ada",
                file = "ada.vcf.1790000000.conflict"
            )
        );

        let read_only = StoreError::ReadOnly("Work".to_owned());
        assert_eq!(
            save_error("Ada", &read_only),
            fl!("error-save", name = "Ada", why = read_only.to_string())
        );
    }

    /// A PNG of the given size, for the photo-processing tests.
    fn png(width: u32, height: u32) -> Vec<u8> {
        let img = image::DynamicImage::ImageRgb8(image::RgbImage::new(width, height));
        let mut out = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
            .unwrap();
        out
    }

    #[test]
    fn a_landscape_photo_is_cropped_square_and_scaled_down() {
        let (data, mime) = process_photo(png(2000, 1000), "image/png");
        assert_eq!(mime, "image/jpeg");
        let img = image::load_from_memory(&data).unwrap();
        assert_eq!((img.width(), img.height()), (PHOTO_SIDE, PHOTO_SIDE));
    }

    #[test]
    fn a_small_portrait_photo_is_cropped_but_not_scaled_up() {
        let (data, _) = process_photo(png(60, 100), "image/png");
        let img = image::load_from_memory(&data).unwrap();
        assert_eq!((img.width(), img.height()), (60, 60));
    }

    /// Re-setting a photo that already fits must not degrade it: the bytes
    /// pass through untouched, generation loss zero.
    #[test]
    fn a_square_small_photo_passes_through_verbatim() {
        let original = png(200, 200);
        let (data, mime) = process_photo(original.clone(), "image/png");
        assert_eq!(data, original);
        assert_eq!(mime, "image/png");
    }

    /// Undecodable bytes are stored as chosen — refusing the photo outright
    /// would be worse than embedding something another client may understand.
    #[test]
    fn undecodable_bytes_pass_through_verbatim() {
        let noise = vec![0xAB; 64];
        let (data, mime) = process_photo(noise.clone(), "image/jpeg");
        assert_eq!(data, noise);
        assert_eq!(mime, "image/jpeg");
    }
}
