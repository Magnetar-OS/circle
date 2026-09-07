// SPDX-License-Identifier: GPL-3.0-only

//! Files kept with a contact — a scanned business card, a signed contract, a
//! photo of a whiteboard.
//!
//! Local only, and loudly so. vCard *can* carry an attachment inline, and
//! doing that would push a multi-megabyte PDF through CardDAV onto a server
//! and into every other client syncing that book. So these live beside the
//! address books in `$contacts_root/.crm/blobs/`, under the same
//! dot-directory the rest of [`crate::crm`] uses — invisible to the vdir
//! scanner, never provisioned, never pushed.
//!
//! # Content-addressed
//!
//! A blob's name is the SHA-256 of its bytes, so attaching the same scan to
//! three people stores it once and attaching it twice to one person is a
//! no-op the code does not have to special-case. The original extension is
//! kept on the file name — the hash is still the identity — because a blob
//! with no extension is a file the desktop's handler cannot open.
//!
//! # Deleting is reference-counted, by looking
//!
//! Removing an attachment removes the reference; the blob goes only when no
//! record still names it. There is no count to keep in sync because the
//! records *are* the count — walking a few dozen JSON files is cheaper than
//! a counter that can drift and orphan a file forever.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

/// One file attached to a card.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Attachment {
    /// The blob's file name — `<sha256>.<ext>`, or just `<sha256>` when the
    /// original had no extension.
    pub blob: String,
    /// What the file was called when it was attached. The blob's own name is
    /// a digest, which is not something to show anybody.
    pub name: String,
    pub added: chrono::DateTime<chrono::Utc>,
    /// Bytes, so the interface can say how big something is without stat-ing
    /// a file per frame.
    pub size: u64,
}

/// The largest file worth keeping beside an address book.
///
/// Not a technical limit — the blob directory would hold more. It is a limit
/// on surprise: an address book that quietly grows by a gigabyte because
/// somebody attached a video is not what anyone asked for, and the message
/// this produces says the number so the choice is the user's.
pub const MAX_BYTES: u64 = 25 * 1024 * 1024;

/// Where blobs live under a contacts root.
#[must_use]
pub fn blob_dir(contacts_root: &Path) -> PathBuf {
    contacts_root.join(".crm").join("blobs")
}

/// Copies a file into the blob directory and describes it.
///
/// Idempotent: a file already stored under its digest is not written again.
pub fn store(contacts_root: &Path, source: &Path) -> Result<Attachment, String> {
    let size = std::fs::metadata(source)
        .map_err(|why| format!("{}: {why}", file_label(source)))?
        .len();
    if size > MAX_BYTES {
        return Err(crate::fl!(
            "attachment-too-big",
            size = format_size(size),
            limit = format_size(MAX_BYTES)
        ));
    }

    let bytes = std::fs::read(source).map_err(|why| format!("{}: {why}", file_label(source)))?;
    let digest = hex(&Sha256::digest(&bytes));
    let blob = match extension(source) {
        Some(ext) => format!("{digest}.{ext}"),
        None => digest,
    };

    let dir = blob_dir(contacts_root);
    std::fs::create_dir_all(&dir).map_err(|why| why.to_string())?;
    let path = dir.join(&blob);
    // Already stored: same bytes, same name, nothing to do. Writing anyway
    // would be harmless and slower.
    if !path.exists() {
        std::fs::write(&path, &bytes).map_err(|why| why.to_string())?;
    }

    Ok(Attachment {
        blob,
        name: file_label(source),
        added: chrono::Utc::now(),
        size,
    })
}

/// A blob's path, whether or not it exists.
#[must_use]
pub fn path(contacts_root: &Path, attachment: &Attachment) -> PathBuf {
    blob_dir(contacts_root).join(&attachment.blob)
}

/// Deletes a blob if `still_referenced` says nothing points at it any more.
///
/// The caller owns the records, so it owns the question; this only acts on
/// the answer.
pub fn prune(contacts_root: &Path, blob: &str, still_referenced: bool) -> Result<(), String> {
    if still_referenced {
        return Ok(());
    }
    let path = blob_dir(contacts_root).join(blob);
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(()),
        Err(why) if why.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(why) => Err(why.to_string()),
    }
}

/// Deletes every blob no record names any more.
///
/// Deleting a contact deliberately does **not** prune: the delete is
/// undoable, the record travels in the undo entry, and a blob deleted at that
/// moment could not be brought back — the bytes are gone. So orphans are
/// swept at start-up instead, when nothing is mid-undo.
///
/// Returns how many were removed, for the log.
pub fn prune_orphans(
    contacts_root: &Path,
    referenced: &std::collections::HashSet<String>,
) -> usize {
    let dir = blob_dir(contacts_root);
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return 0;
    };
    let mut removed = 0;
    for entry in entries.filter_map(Result::ok) {
        let Some(name) = entry.file_name().to_str().map(ToOwned::to_owned) else {
            continue;
        };
        if referenced.contains(&name) {
            continue;
        }
        match std::fs::remove_file(entry.path()) {
            Ok(()) => removed += 1,
            Err(why) => {
                tracing::warn!(blob = name, %why, "could not remove an orphaned attachment")
            }
        }
    }
    removed
}

/// A lowercase extension, if the name has a usable one.
///
/// Long or odd extensions are dropped rather than carried into a file name:
/// the extension is a hint for the desktop's handler, not data to preserve,
/// and the original name is kept in the record either way.
fn extension(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?.to_lowercase();
    (!ext.is_empty() && ext.len() <= 8 && ext.chars().all(|c| c.is_ascii_alphanumeric()))
        .then_some(ext)
}

fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// A path's file name, for showing to a reader.
fn file_label(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// A byte count somebody can read.
#[must_use]
pub fn format_size(bytes: u64) -> String {
    #[allow(clippy::cast_precision_loss)]
    let n = bytes as f64;
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", n / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.0} kB", n / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(dir: &Path, name: &str, bytes: &[u8]) -> PathBuf {
        let path = dir.join(name);
        std::fs::write(&path, bytes).unwrap();
        path
    }

    #[test]
    fn a_stored_file_lands_in_the_blob_directory_under_its_digest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let file = source(dir.path(), "scan.pdf", b"a business card");

        let attachment = store(&root, &file).unwrap();
        assert_eq!(attachment.name, "scan.pdf");
        assert_eq!(attachment.size, 15);
        assert!(attachment.blob.ends_with(".pdf"), "{}", attachment.blob);
        assert_eq!(attachment.blob.len(), 64 + 4, "not a sha-256 hex name");
        assert!(path(&root, &attachment).exists());
    }

    /// The point of content addressing: the same file attached twice is one
    /// blob, with no special case anywhere.
    #[test]
    fn the_same_bytes_are_stored_once() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let a = store(&root, &source(dir.path(), "one.pdf", b"same")).unwrap();
        let b = store(&root, &source(dir.path(), "two.pdf", b"same")).unwrap();

        assert_eq!(a.blob, b.blob);
        assert_eq!(std::fs::read_dir(blob_dir(&root)).unwrap().count(), 1);
    }

    #[test]
    fn different_bytes_are_different_blobs() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let a = store(&root, &source(dir.path(), "a.pdf", b"one")).unwrap();
        let b = store(&root, &source(dir.path(), "b.pdf", b"two")).unwrap();

        assert_ne!(a.blob, b.blob);
        assert_eq!(std::fs::read_dir(blob_dir(&root)).unwrap().count(), 2);
    }

    /// A blob nothing points at any more is deleted; one that is still
    /// referenced is not, or attaching a scan to two people and detaching it
    /// from one would break the other.
    #[test]
    fn pruning_respects_a_remaining_reference() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let a = store(&root, &source(dir.path(), "scan.pdf", b"shared")).unwrap();

        prune(&root, &a.blob, true).unwrap();
        assert!(
            path(&root, &a).exists(),
            "deleted a blob somebody still uses"
        );

        prune(&root, &a.blob, false).unwrap();
        assert!(!path(&root, &a).exists(), "kept a blob nobody references");
    }

    #[test]
    fn pruning_a_blob_that_is_already_gone_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        prune(&dir.path().join("contacts"), "deadbeef.pdf", false).unwrap();
    }

    #[test]
    fn a_file_over_the_limit_is_refused_with_both_numbers() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let big = source(dir.path(), "huge.bin", &vec![0u8; (MAX_BYTES + 1) as usize]);

        let why = store(&root, &big).unwrap_err();
        assert!(why.contains("MB"), "the message should say the size: {why}");
        assert!(
            !blob_dir(&root).exists(),
            "a refused file was written anyway"
        );
    }

    #[test]
    fn a_file_with_no_usable_extension_is_stored_under_the_bare_digest() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let a = store(&root, &source(dir.path(), "notes", b"x")).unwrap();
        assert_eq!(a.blob.len(), 64);
        assert_eq!(a.name, "notes");
    }

    /// An extension is a hint for the desktop handler, and one carrying
    /// punctuation has no business in a generated file name.
    #[test]
    fn an_odd_extension_is_dropped_rather_than_carried() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let a = store(&root, &source(dir.path(), "x.this-is-not-an-ext", b"x")).unwrap();
        assert_eq!(a.blob.len(), 64, "kept a junk extension: {}", a.blob);
    }

    /// A contact deleted without an undo leaves its blobs behind; the sweep
    /// is what stops the directory growing forever.
    #[test]
    fn the_sweep_removes_orphans_and_keeps_what_is_referenced() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        let kept = store(&root, &source(dir.path(), "kept.pdf", b"kept")).unwrap();
        let orphan = store(&root, &source(dir.path(), "gone.pdf", b"orphan")).unwrap();

        let referenced: std::collections::HashSet<String> =
            std::iter::once(kept.blob.clone()).collect();
        assert_eq!(prune_orphans(&root, &referenced), 1);

        assert!(path(&root, &kept).exists(), "swept a referenced blob");
        assert!(!path(&root, &orphan).exists(), "kept an orphan");
    }

    #[test]
    fn sweeping_a_directory_that_does_not_exist_is_not_an_error() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            prune_orphans(
                &dir.path().join("contacts"),
                &std::collections::HashSet::new()
            ),
            0
        );
    }

    #[test]
    fn sizes_read_the_way_people_write_them() {
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(2048), "2 kB");
        assert_eq!(format_size(5 * 1024 * 1024), "5.0 MB");
    }

    /// Blobs sit under the same dot-directory as the rest of the CRM data, so
    /// the vdir scanner cannot see them.
    #[test]
    fn the_blob_directory_never_becomes_an_address_book() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("contacts");
        std::fs::create_dir_all(&root).unwrap();
        store(&root, &source(dir.path(), "scan.pdf", b"x")).unwrap();

        assert!(cosmic_pim_core::store::contacts::books(&root).is_empty());
    }
}
