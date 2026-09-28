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
//! - **Merge**: when the sync pass kept the revision both sides started from,
//!   [`cosmic_pim_core::merge`] lists the properties both sides changed; the
//!   user picks a side for each and every other property keeps both edits.
//!   Two edits that touch different properties merge without a question.

use std::collections::BTreeMap;
use std::path::Path;

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
    /// Who this device's copy says the card is.
    pub yours: String,
    /// Who the server's copy says the card is.
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
            let disputes = conflict.base.as_deref().and_then(|base| {
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
                yours: describe(&conflict.local, &conflict.href),
                theirs: describe(&conflict.remote, &conflict.href),
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

/// A card text's name, for "yours" and "theirs". The href's last segment
/// when the text carries no name at all, so a row is never blank.
fn describe(text: &str, href: &str) -> String {
    cosmic_pim_core::vcard::parse_vcards(text, "", "")
        .first()
        .map(cosmic_pim_core::model::Contact::label)
        .filter(|label| !label.is_empty())
        .unwrap_or_else(|| {
            href.rsplit('/')
                .find(|s| !s.is_empty())
                .unwrap_or(href)
                .to_owned()
        })
}
