// SPDX-License-Identifier: GPL-3.0-only

//! Sync conflicts: cards this device and the server both changed.
//!
//! The substrate detects them, parks both texts and leaves the local file
//! alone (`cosmic_pim_sync::conflict`); until somebody decides, the local edit
//! stays local and its push stays parked. This module is the view over that
//! record and the three ways out, nothing more — detection, storage and the
//! merge itself belong to the substrate, the same split Slate uses for events.
//!
//! - **Keep mine**: this device's card wins and is re-queued for upload.
//! - **Take the server's**: the server's card is written locally and the
//!   parked push is dropped.
//!
//! One side may be a deletion rather than an edit ([`ConflictKind`]): the
//! server deleted a card this device changed, or this device deleted a card
//! the server changed. The same two answers apply — the substrate does the
//! right thing for each kind — but they mean different things to the person
//! choosing, so each kind is worded for what the buttons will actually do.
//! - **Merge**: when the sync pass kept the revision both sides started from,
//!   [`cosmic_pim_core::merge`] lists the properties both sides changed; the
//!   user picks a side for each and every other property keeps both edits.
//!   Two edits that touch different properties merge without a question.

use std::collections::BTreeMap;
use std::path::Path;

pub use cosmic_pim_caldav::ConflictKind;
use cosmic_pim_core::merge::{self, Overlap, Side};

/// One unresolved conflict, shaped for the Accounts page.
#[derive(Clone, Debug)]
pub struct ConflictRow {
    /// The address book (vdir collection) it is in.
    pub book: String,
    /// That book's display name.
    pub book_name: String,
    /// The card's href on the server — the conflict's identity.
    pub href: String,
    /// Which two changes collided: two edits, or an edit and a deletion.
    pub kind: ConflictKind,
    /// Who this device's copy says the card is. For
    /// [`ConflictKind::DeletedHere`] there is no copy here, and this is the
    /// server's name for them.
    pub yours: String,
    /// Who the server's copy says the card is. For
    /// [`ConflictKind::DeletedOnServer`] there is no copy there, and this is
    /// this device's name for them.
    pub theirs: String,
    /// Per-property material, when the base revision was recorded and the
    /// texts are comparable. `None` leaves only the wholesale answers.
    pub disputes: Option<Disputes>,
}

/// The three-way merge material for one conflict.
#[derive(Clone, Debug)]
pub struct Disputes {
    base: String,
    local: String,
    remote: String,
    /// The properties both sides changed incompatibly. Empty means the two
    /// edits touch different properties and merge without asking anything.
    pub units: Vec<Overlap>,
    /// The side chosen so far for each disputed property, keyed by
    /// [`Overlap::unit`].
    pub choices: BTreeMap<String, Side>,
}

impl Disputes {
    /// Whether every disputed property has an answer.
    #[must_use]
    pub fn decided(&self) -> bool {
        self.units
            .iter()
            .all(|u| self.choices.contains_key(&u.unit))
    }

    /// Records the side chosen for the `index`-th disputed property.
    pub fn choose(&mut self, index: usize, side: Side) {
        if let Some(unit) = self.units.get(index) {
            self.choices.insert(unit.unit.clone(), side);
        }
    }

    /// The merged card, once every disputed property is decided.
    #[must_use]
    pub fn merged(&self) -> Option<String> {
        if !self.decided() {
            return None;
        }
        merge::resolve(&self.base, &self.local, &self.remote, &self.choices)
    }
}

/// How the user answered one conflict.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Resolution {
    KeepMine,
    TakeTheirs,
    /// The merged card from [`Disputes::merged`].
    Merge,
}

/// Every unresolved conflict under a contacts root.
#[must_use]
pub fn load(root: &Path) -> Vec<ConflictRow> {
    let books = cosmic_pim_core::store::contacts::books(root);
    cosmic_pim_sync::conflicts(root)
        .into_iter()
        .map(|(book, conflict)| {
            // Per-property choice only makes sense between two edits; with a
            // deletion on one side there is nothing to merge.
            let both_edited = conflict.kind == ConflictKind::BothEdited;
            let disputes = conflict
                .base
                .as_deref()
                .filter(|_| both_edited)
                .and_then(|base| {
                    merge::overlaps(base, &conflict.local, &conflict.remote).map(|units| Disputes {
                        base: base.to_owned(),
                        local: conflict.local.clone(),
                        remote: conflict.remote.clone(),
                        units,
                        choices: BTreeMap::new(),
                    })
                });
            ConflictRow {
                book_name: books
                    .iter()
                    .find(|b| b.id == book)
                    .map_or_else(|| book.clone(), |b| b.name.clone()),
                yours: describe(&conflict.local, &conflict.remote, &conflict.href),
                theirs: describe(&conflict.remote, &conflict.local, &conflict.href),
                kind: conflict.kind,
                book,
                href: conflict.href,
                disputes,
            }
        })
        .collect()
}

/// Applies the user's answer to one conflict.
///
/// # Errors
///
/// The substrate's error when the resolution could not be written, or
/// `merge-failed` when [`Resolution::Merge`] was asked of a conflict whose
/// merge cannot be built — undecided, or texts that stopped being comparable.
pub fn resolve(root: &Path, row: &ConflictRow, how: Resolution) -> Result<(), ResolveError> {
    let outcome = match how {
        Resolution::KeepMine => {
            cosmic_pim_sync::conflict::keep_local(root, &row.book, &row.href, None)
        }
        Resolution::TakeTheirs => {
            cosmic_pim_sync::conflict::take_remote(root, &row.book, &row.href)
        }
        Resolution::Merge => {
            let merged = row
                .disputes
                .as_ref()
                .and_then(Disputes::merged)
                .ok_or(ResolveError::MergeFailed)?;
            cosmic_pim_sync::conflict::keep_local(root, &row.book, &row.href, Some(&merged))
        }
    };
    outcome
        .map(|_| ())
        .map_err(|why| ResolveError::Write(why.to_string()))
}

/// Why a resolution did not happen.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ResolveError {
    /// The merged card could not be built; a whole side is still available.
    MergeFailed,
    /// The substrate could not write the resolution.
    Write(String),
}

/// A card text's name, for "yours" and "theirs". When that side is a
/// deletion its text is empty, and the other side names the card; the href's
/// last segment is the last resort, so a row is never blank.
fn describe(text: &str, other: &str, href: &str) -> String {
    let name = |text: &str| {
        cosmic_pim_core::vcard::parse_vcards(text, "", "")
            .first()
            .map(cosmic_pim_core::model::Contact::label)
            .filter(|label| !label.is_empty())
    };
    name(text).or_else(|| name(other)).unwrap_or_else(|| {
        href.rsplit('/')
            .find(|s| !s.is_empty())
            .unwrap_or(href)
            .to_owned()
    })
}

/// What a conflict row says, and what its two buttons say, by kind.
///
/// "Keep mine" and "Take the server's" are exact for two edits and
/// misleading for a deletion: keeping this device's side of a
/// [`ConflictKind::DeletedHere`] conflict *deletes* the card on the server.
#[must_use]
pub fn wording(row: &ConflictRow) -> Wording {
    match row.kind {
        ConflictKind::BothEdited => Wording {
            summary: crate::fl!(
                "conflict-versions",
                yours = row.yours.clone(),
                theirs = row.theirs.clone()
            ),
            keep_mine: crate::fl!("conflict-keep-mine"),
            take_theirs: crate::fl!("conflict-take-theirs"),
        },
        ConflictKind::DeletedOnServer => Wording {
            summary: crate::fl!("conflict-deleted-on-server", name = row.yours.clone()),
            keep_mine: crate::fl!("conflict-keep-and-restore-on-server"),
            take_theirs: crate::fl!("conflict-delete-here-too"),
        },
        ConflictKind::DeletedHere => Wording {
            summary: crate::fl!("conflict-deleted-here", name = row.theirs.clone()),
            keep_mine: crate::fl!("conflict-delete-on-server-too"),
            take_theirs: crate::fl!("conflict-restore-servers"),
        },
    }
}

/// The text of one conflict row: see [`wording`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wording {
    pub summary: String,
    /// The label for [`Resolution::KeepMine`].
    pub keep_mine: String,
    /// The label for [`Resolution::TakeTheirs`].
    pub take_theirs: String,
}
