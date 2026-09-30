// SPDX-License-Identifier: GPL-3.0-only

//! Accounts and sync: running a pass, saying what it left behind, and
//! queueing local writes for upload.

use std::path::Path;

use cosmic::app::Task;
use cosmic::widget;
use cosmic_pim_core::store::StoreError;
use cosmic_pim_core::store::contacts::ContactStore;

use super::{AppModel, ContextPage, Message};
use crate::fl;

impl AppModel {
    /// Validates the add-account form and stores the account.
    pub(super) fn confirm_account(&mut self) -> Task<Message> {
        let Some(form) = self.account_form.clone() else {
            return Task::none();
        };
        let Some(accounts) = self.accounts.as_mut() else {
            return self.toast(fl!("error-no-account-store"));
        };

        let url = form.url.trim();
        // Refuse plaintext up front rather than after the password has been
        // typed, stored, and sent: a CardDAV password over http is compromised
        // the first time it is used, and no later warning undoes that.
        if !url.starts_with("https://") && !url.starts_with("http://") {
            self.with_account_form(|f| f.error = Some(fl!("error-url-scheme")));
            return Task::none();
        }
        if url.starts_with("http://") && !is_loopback(url) {
            self.with_account_form(|f| f.error = Some(fl!("error-url-insecure")));
            return Task::none();
        }

        let display_name = if form.display_name.trim().is_empty() {
            form.username.trim().to_owned()
        } else {
            form.display_name.trim().to_owned()
        };

        let account = cosmic_pim_accounts::Account::new(&display_name, url, form.username.trim());

        match accounts.add(account, &form.password) {
            Ok(()) => {
                self.account_form = None;
                // Sync immediately: the user just told us where their
                // contacts are, and waiting for a timer to act on that feels
                // broken.
                self.sync_now()
            }
            Err(why) => {
                self.with_account_form(|f| f.error = Some(why.to_string()));
                Task::none()
            }
        }
    }

    /// The toast for a pass that left something to look at, raised only
    /// when the set of flagged accounts changed and the Accounts page — where
    /// the details are — is not already showing.
    pub(super) fn sync_alert(&mut self, attention: Vec<String>) -> Task<Message> {
        let changed = attention != self.sync_attention;
        self.sync_attention = attention;
        let page_open = self.core.window.show_context && self.context_page == ContextPage::Accounts;
        if !changed || page_open || self.sync_attention.is_empty() {
            return Task::none();
        }
        let message = fl!(
            "sync-needs-attention",
            accounts = self.sync_attention.join(", ")
        );
        self.toasts
            .push(widget::Toast::new(message).action(fl!("accounts"), |_| Message::ShowAccounts))
            .map(Into::into)
    }

    /// Re-reads the shared account store before the Accounts page shows it.
    ///
    /// The handle lives as long as the window, and Slate, Envelope and every
    /// sync pass write the same `accounts.toml`; a list from start-up would
    /// show accounts removed elsewhere and miss ones added there.
    pub(super) fn reload_accounts(&mut self) -> Task<Message> {
        match self.accounts.as_mut().map(reload_account_store) {
            Some(Err(why)) => self.toast(why),
            _ => Task::none(),
        }
    }

    /// Re-reads the unresolved conflicts from the sync sidecars.
    pub(super) fn reload_conflicts(&mut self) {
        self.conflicts = crate::conflicts::load(&self.contacts_root);
    }

    /// Applies the user's answer to one conflict, then re-reads everything
    /// the resolution may have rewritten.
    pub(super) fn resolve_conflict(
        &mut self,
        index: usize,
        how: crate::conflicts::Resolution,
    ) -> Task<Message> {
        let Some(row) = self.conflicts.get(index) else {
            return Task::none();
        };
        match crate::conflicts::resolve(&self.contacts_root, row, how) {
            Ok(()) => {
                if let Some(store) = self.store.as_mut() {
                    store.refresh();
                }
                self.photos.clear();
                self.rebuild_nav();
                self.reload();
                self.reload_conflicts();
                Task::none()
            }
            Err(crate::conflicts::ResolveError::MergeFailed) => {
                self.toast(fl!("conflict-merge-failed"))
            }
            Err(crate::conflicts::ResolveError::Write(why)) => self.toast(why),
        }
    }

    /// Runs a sync pass off the UI thread.
    pub(super) fn sync_now(&mut self) -> Task<Message> {
        if self.syncing || self.accounts.is_none() {
            return Task::none();
        }
        self.syncing = true;
        self.sync_status = None;

        // The sync engine walks CalDAV and CardDAV in one pass: an account can
        // offer both, and the calendars land in the suite's calendar root for
        // Slate to read — the mirror image of Slate's own sync pass filling
        // the contacts root for Circle.
        let calendar_root = cosmic_pim_core::store::vdir::default_root();
        let contacts_root = self
            .store
            .as_ref()
            .map_or_else(cosmic_pim_core::store::contacts::default_root, |store| {
                store.root().to_path_buf()
            });
        // Provider manifests: how an account that names a provider rather than
        // a raw URL resolves its endpoints and OAuth client.
        let registry = cosmic_pim_accounts::Registry::load();
        cosmic::task::future(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                // Reopened inside the task: `AccountStore` is not shared with
                // the UI thread, and re-reading also picks up any change made
                // since the button was pressed.
                let mut accounts = match cosmic_pim_accounts::AccountStore::open_default() {
                    Ok(accounts) => accounts,
                    Err(why) => return SyncSummary::failed(why.to_string()),
                };
                let reports = cosmic_pim_sync::sync_all(
                    &mut accounts,
                    &registry,
                    &calendar_root,
                    &contacts_root,
                );
                SyncSummary::of(&reports)
            })
            .await
            .unwrap_or_else(|why| SyncSummary::failed(why.to_string()));

            Message::SyncFinished(outcome)
        })
    }
}

/// [`cosmic_pim_accounts::AccountStore::reload`], with the failure as a
/// sentence for a toast.
fn reload_account_store(accounts: &mut cosmic_pim_accounts::AccountStore) -> Result<(), String> {
    accounts
        .reload()
        .map_err(|why| format!("{}: {why}", fl!("accounts")))
}

/// Writes to a book and queues what the write left for upload, as one step.
///
/// Storage deliberately knows nothing about CardDAV (see
/// `cosmic_pim_sync::writeback`), so every write site in the shell goes
/// through here. `write` is the change — a save, a delete, a restore — and
/// `file_names` the files of `book_id` it may touch. Through
/// [`cosmic_pim_sync::save_and_queue`] the write and its enqueue happen under
/// the book's sync lock: a sync pass pulling the same card can no longer land
/// between the two and write the server's copy over the edit. The bytes each
/// file held before the write are the base the sync engine merges against
/// when the server changed the same card; a file the write removed is queued
/// as a deletion, and one that still holds other cards as an upload.
///
/// `Err` means nothing was written — the write failed, or the book's sync
/// state could not be opened and the write was not tried. `Ok` carries the
/// write's value and whether its upload was queued: a failure to queue does
/// not undo the local write — the card's text is not at risk — but comes
/// back as a sentence for a toast, because the edit will not reach the server
/// until the card is written again, and nobody would otherwise know. A
/// local-only book queues nothing and is `Ok(())`.
///
/// `write` must not queue anything itself: the lock is held around it.
pub(super) fn write_and_queue<T>(
    root: &Path,
    book_id: &str,
    file_names: &[&str],
    write: impl FnOnce() -> Result<T, StoreError>,
) -> Result<(T, Result<(), String>), cosmic_pim_sync::Error> {
    let saved = cosmic_pim_sync::save_and_queue(root, book_id, file_names, write)?;
    let queued = saved
        .queued
        .map(|_| ())
        .map_err(|why| fl!("error-queue-upload", why = why.to_string()));
    Ok((saved.value, queued))
}

/// Queues a file a write has just *created* for upload.
///
/// For the writes that choose their file's name themselves — a new group
/// card, the new cards of an import — so the name is not known until the
/// write returns and [`write_and_queue`] cannot be handed it. Queueing after
/// the write is safe for these: a sync pass only pulls what the server
/// lists, and the server has no resource by a name this device just made
/// up. A failure to queue is returned as a sentence for a toast, as there.
pub(super) fn queue_created(
    store: &ContactStore,
    book_id: &str,
    file_name: &str,
) -> Result<(), String> {
    cosmic_pim_sync::queue_save(store.root(), book_id, file_name)
        .map(|_| ())
        .map_err(|why| fl!("error-queue-upload", why = why.to_string()))
}

/// What a sync pass tells the UI thread.
#[derive(Clone, Debug)]
pub struct SyncSummary {
    /// One line per account, for the Accounts page.
    pub lines: Vec<String>,
    /// Whether anything landed on disk.
    pub changed: bool,
    /// The accounts whose contacts need a person — see
    /// [`contacts_need_attention`].
    pub attention: Vec<String>,
}

impl SyncSummary {
    fn of(reports: &[cosmic_pim_sync::AccountReport]) -> Self {
        Self {
            lines: reports
                .iter()
                .map(|r| status_line(&r.display_name, &r.tally()))
                .collect(),
            changed: reports.iter().any(cosmic_pim_sync::AccountReport::changed),
            attention: reports
                .iter()
                .filter(|r| contacts_need_attention(r))
                .map(|r| r.display_name.clone())
                .collect(),
        }
    }

    /// The pass could not start at all.
    fn failed(why: String) -> Self {
        Self {
            lines: vec![why.clone()],
            changed: false,
            attention: vec![why],
        }
    }
}

/// Whether a pass left this account's contacts needing a person: the account
/// failed outright, its address books refused us, or an address book failed,
/// holds a conflict, or has edits parked on a password, permission or quota.
///
/// Calendars are Slate's to report. Circle syncs them in the same pass, but a
/// calendar that failed is not something the contacts app should interrupt
/// anyone about.
fn contacts_need_attention(report: &cosmic_pim_sync::AccountReport) -> bool {
    if report.contacts_unavailable.is_some() {
        return true;
    }
    match &report.collections {
        Err(_) => true,
        Ok(collections) => collections.iter().any(|c| {
            c.flavor == cosmic_pim_caldav::Flavor::CardDav
                && (c.outcome.is_err() || c.needs_attention())
        }),
    }
}

/// One account's line on the Accounts page after a sync pass, worded from
/// the substrate's [`cosmic_pim_sync::SyncTally`] in the interface language.
///
/// The address-book half is part of the tally, so an account whose CardDAV
/// side refused us never reads "up to date" on the strength of its
/// calendars.
fn status_line(account: &str, tally: &cosmic_pim_sync::SyncTally) -> String {
    if let Some(why) = &tally.account_error {
        return fl!("sync-line", account = account, details = why.clone());
    }
    let mut parts = Vec::new();
    for (count, id) in [
        (tally.fetched, "sync-fetched"),
        (tally.deleted, "sync-deleted"),
        (tally.pushed, "sync-pushed"),
        (tally.failed, "sync-failed"),
        (tally.conflicts, "sync-conflicts"),
        (tally.held, "sync-held"),
    ] {
        if count > 0 {
            parts.push(match id {
                "sync-fetched" => fl!("sync-fetched", count = count),
                "sync-deleted" => fl!("sync-deleted", count = count),
                "sync-pushed" => fl!("sync-pushed", count = count),
                "sync-failed" => fl!("sync-failed", count = count),
                "sync-conflicts" => fl!("sync-conflicts", count = count),
                _ => fl!("sync-held", count = count),
            });
        }
    }
    if let Some(why) = &tally.contacts_unavailable {
        parts.push(fl!("sync-contacts-unreachable", why = why.clone()));
    }
    if parts.is_empty() {
        parts.push(fl!("sync-up-to-date"));
    }
    fl!("sync-line", account = account, details = parts.join(", "))
}

/// Whether an `http://` URL points at this machine — the one case where
/// sending a password unencrypted is acceptable, because it never leaves it.
fn is_loopback(url: &str) -> bool {
    let host = url
        .trim_start_matches("http://")
        .split(['/', ':'])
        .next()
        .unwrap_or_default();
    host == "localhost" || host == "127.0.0.1" || host == "::1"
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An account whose calendars synced but whose address books could not be
    /// reached must not read "up to date": in the contacts app, that is the
    /// one account whose contacts did not sync at all.
    #[test]
    fn an_account_whose_address_books_were_not_reached_says_so() {
        let tally = cosmic_pim_sync::SyncTally {
            contacts_unavailable: Some("HTTP 401 Unauthorized".to_owned()),
            ..Default::default()
        };
        let line = status_line("Fastmail", &tally);
        assert!(line.contains("Fastmail"), "{line}");
        assert!(line.contains("HTTP 401 Unauthorized"), "{line}");
        assert!(!line.contains(&fl!("sync-up-to-date")), "{line}");
    }

    /// Each count is worded from the catalogue, and a quiet pass says so.
    #[test]
    fn the_status_line_is_worded_from_the_catalogue() {
        let quiet = status_line("Fastmail", &cosmic_pim_sync::SyncTally::default());
        assert!(quiet.contains(&fl!("sync-up-to-date")), "{quiet}");

        let busy = cosmic_pim_sync::SyncTally {
            fetched: 3,
            conflicts: 1,
            ..Default::default()
        };
        let line = status_line("Fastmail", &busy);
        assert!(line.contains(&fl!("sync-fetched", count = 3)), "{line}");
        assert!(line.contains(&fl!("sync-conflicts", count = 1)), "{line}");
        assert!(!line.contains(&fl!("sync-up-to-date")), "{line}");

        let failed = cosmic_pim_sync::SyncTally {
            account_error: Some("host is down".to_owned()),
            ..Default::default()
        };
        assert!(status_line("Fastmail", &failed).contains("host is down"));
    }

    fn account(
        collections: cosmic_pim_sync::Result<Vec<cosmic_pim_sync::CollectionReport>>,
    ) -> cosmic_pim_sync::AccountReport {
        cosmic_pim_sync::AccountReport {
            account_id: "acct".to_owned(),
            display_name: "Fastmail".to_owned(),
            collections,
            contacts_unavailable: None,
        }
    }

    fn collection(
        flavor: cosmic_pim_caldav::Flavor,
        outcome: cosmic_pim_sync::Result<cosmic_pim_caldav::SyncOutcome>,
    ) -> cosmic_pim_sync::CollectionReport {
        cosmic_pim_sync::CollectionReport {
            collection_id: "c".to_owned(),
            display_name: "C".to_owned(),
            href: "/c/".to_owned(),
            flavor,
            pushed: cosmic_pim_caldav::push::DrainOutcome::default(),
            outcome,
        }
    }

    fn refused() -> cosmic_pim_sync::Error {
        cosmic_pim_sync::Error::ForeignSyncOwner {
            collection: "C".to_owned(),
            marker: ".vdirsyncer".to_owned(),
        }
    }

    /// Sync failures reached nobody unless the Accounts page happened to be
    /// open (ROADMAP A1: "never silent"). These are the ones that interrupt.
    #[test]
    fn a_contacts_failure_or_conflict_needs_attention() {
        use cosmic_pim_caldav::{Flavor, SyncOutcome};

        assert!(contacts_need_attention(&account(Err(refused()))));

        let failed = account(Ok(vec![collection(Flavor::CardDav, Err(refused()))]));
        assert!(contacts_need_attention(&failed));

        let conflicted = account(Ok(vec![collection(
            Flavor::CardDav,
            Ok(SyncOutcome {
                conflicts: 1,
                ..SyncOutcome::default()
            }),
        )]));
        assert!(contacts_need_attention(&conflicted));

        let mut refused_books = account(Ok(Vec::new()));
        refused_books.contacts_unavailable = Some("HTTP 401".to_owned());
        assert!(contacts_need_attention(&refused_books));
    }

    /// A calendar failing is Slate's to report; a clean pass is silent.
    #[test]
    fn a_calendar_failure_or_a_clean_pass_does_not() {
        use cosmic_pim_caldav::{Flavor, SyncOutcome};

        let calendar = account(Ok(vec![collection(Flavor::CalDav, Err(refused()))]));
        assert!(!contacts_need_attention(&calendar));

        let clean = account(Ok(vec![collection(
            Flavor::CardDav,
            Ok(SyncOutcome::default()),
        )]));
        assert!(!contacts_need_attention(&clean));

        let summary = SyncSummary::of(&[calendar, clean]);
        assert!(summary.attention.is_empty());
    }

    /// Deleting one card out of a synced file that holds two: the resource
    /// on the server is the file, and the other card is still in it. A
    /// DELETE for the href took the other person off the server too.
    #[test]
    fn deleting_one_card_of_a_shared_synced_file_uploads_the_rest() {
        use cosmic_pim_caldav::push::{PushOp, PushQueue as _};
        use cosmic_pim_caldav::{CalDavStore as _, RemoteEvent, VdirStore};

        const TWO: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n\
BEGIN:VCARD\r\nVERSION:3.0\r\nUID:bob\r\nFN:Bob\r\nEND:VCARD\r\n";
        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let book = store
            .create_book("Synced", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        let mut vdir = VdirStore::open_carddav(book.clone()).unwrap();
        vdir.set_remote("/dav/ab/", false).unwrap();
        vdir.upsert(&RemoteEvent {
            href: "/dav/ab/both.vcf".into(),
            etag: "\"v1\"".into(),
            ics: TWO.into(),
        })
        .unwrap();
        store.refresh();

        let ada = store.contact(&book.id, "ada").unwrap();
        let root = store.root().to_path_buf();
        let ((), queued) = write_and_queue(&root, &book.id, &[&ada.file_name], || {
            store.delete(&book.id, "ada")
        })
        .unwrap();
        queued.unwrap();

        let pending = VdirStore::open(book).unwrap().pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(
            matches!(pending[0].op, PushOp::Put { .. }),
            "queued {:?} for a file that still holds Bob",
            pending[0].op
        );
        assert_eq!(
            pending[0].base.as_deref(),
            Some(TWO),
            "the upload lost the file's pre-delete text, its merge base"
        );
    }

    /// The ordinary case: the card was the whole file, so the resource goes.
    #[test]
    fn deleting_a_card_that_was_its_whole_file_deletes_the_resource() {
        use cosmic_pim_caldav::push::{PushOp, PushQueue as _};
        use cosmic_pim_caldav::{CalDavStore as _, RemoteEvent, VdirStore};

        const ONE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n";
        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let book = store
            .create_book("Synced", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        let mut vdir = VdirStore::open_carddav(book.clone()).unwrap();
        vdir.set_remote("/dav/ab/", false).unwrap();
        vdir.upsert(&RemoteEvent {
            href: "/dav/ab/ada.vcf".into(),
            etag: "\"v1\"".into(),
            ics: ONE.into(),
        })
        .unwrap();
        store.refresh();

        let ada = store.contact(&book.id, "ada").unwrap();
        let root = store.root().to_path_buf();
        let ((), queued) = write_and_queue(&root, &book.id, &[&ada.file_name], || {
            store.delete(&book.id, "ada")
        })
        .unwrap();
        queued.unwrap();

        let pending = VdirStore::open(book).unwrap().pending().unwrap();
        assert_eq!(pending.len(), 1);
        assert!(matches!(pending[0].op, PushOp::Delete { .. }));
    }

    /// A book whose sync state cannot be read: a change that could never be
    /// queued is refused before it is made, and a created file that cannot
    /// be queued is reported. The helpers used to log that and carry on, so
    /// nobody knew the edit would never reach the server.
    #[test]
    fn a_write_that_cannot_be_queued_is_refused_or_reported() {
        use cosmic_pim_core::store::contacts::write_contact_raw;

        const ONE: &str = "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n";
        let dir = tempfile::tempdir().unwrap();
        let mut store = ContactStore::open(dir.path()).unwrap();
        let book = store
            .create_book("Synced", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        // The sync state cannot be read: its path is a directory.
        std::fs::create_dir(book.path.join(".caldav-state.json")).unwrap();

        let refused = write_and_queue(store.root(), &book.id, &["ada.vcf"], || {
            write_contact_raw(&book, "ada.vcf", ONE)
        });
        assert!(refused.is_err());
        assert!(
            !book.path.join("ada.vcf").exists(),
            "the write went ahead with nothing to queue it"
        );

        write_contact_raw(&book, "new.vcf", ONE).unwrap();
        let err = queue_created(&store, &book.id, "new.vcf").unwrap_err();
        assert!(!err.is_empty());
    }

    /// An account added by Slate while Circle is open shows up on Circle's
    /// Accounts page, and one removed there disappears: the page re-reads
    /// the shared store instead of trusting the start-up snapshot.
    #[test]
    fn the_account_list_follows_changes_made_by_another_app() {
        use cosmic_pim_accounts::{Account, AccountStore, SecretStore};

        let dir = tempfile::tempdir().unwrap();
        let open = || {
            AccountStore::open(
                &dir.path().join("accounts.toml"),
                SecretStore::open_envelope_only("circle-test", dir.path()),
            )
            .unwrap()
        };
        let mut circle = open();
        let mut slate = open();

        let account = Account::new("Fastmail", "https://dav.example.com", "ada");
        let id = account.id.clone();
        slate.add(account, "secret").unwrap();
        assert!(
            circle.accounts().is_empty(),
            "a stale handle saw the change"
        );
        reload_account_store(&mut circle).unwrap();
        assert_eq!(circle.accounts().len(), 1);

        slate.remove(&id).unwrap();
        reload_account_store(&mut circle).unwrap();
        assert!(circle.accounts().is_empty());
    }
}
