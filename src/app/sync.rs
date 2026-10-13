// SPDX-License-Identifier: GPL-3.0-only

//! Accounts and sync: running a pass, saying what it left behind, and
//! queueing local writes for upload.

use std::path::Path;

use cosmic::app::Task;
use cosmic::widget;
use cosmic_pim_core::store::StoreError;
use cosmic_pim_core::store::contacts::ContactStore;

use super::{AccountForm, AppModel, ContextPage, Message, ServerNotAdded};
use crate::fl;

impl AppModel {
    /// Validates the add-account form, and on a worker stores the account
    /// once the server has taken the login.
    pub(super) fn confirm_account(&mut self) -> Task<Message> {
        let Some(form) = self.account_form.clone() else {
            return Task::none();
        };
        if form.adding {
            return Task::none();
        }
        if self.accounts.is_none() {
            return self.toast(fl!("error-no-account-store"));
        }

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

        self.with_account_form(|f| f.adding = true);
        let registry = self.registry.clone();
        cosmic::task::future(async move {
            let result = tokio::task::spawn_blocking(move || {
                let mut store = cosmic_pim_accounts::AccountStore::open_default()
                    .map_err(|why| ServerNotAdded::Failed(why.to_string()))?;
                add_server(&mut store, &registry, &form).map(drop)
            })
            .await
            .unwrap_or_else(|why| Err(ServerNotAdded::Failed(why.to_string())));
            Message::AccountAddFinished(result)
        })
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

    /// The toast for changes saved here but not queued for upload, with a
    /// Retry that queues them again ([`requeue`]).
    pub(super) fn toast_unqueued(&mut self, unqueued: Unqueued) -> Task<Message> {
        let message = unqueued.to_string();
        self.toasts
            .push(
                widget::Toast::new(message).action(fl!("retry-upload"), move |_| {
                    Message::RetryUpload(unqueued.clone())
                }),
            )
            .map(Into::into)
    }

    /// `task`, with the toast for `unqueued` beside it when there is one.
    pub(super) fn also_unqueued(
        &mut self,
        task: Task<Message>,
        unqueued: Option<Unqueued>,
    ) -> Task<Message> {
        match unqueued {
            Some(unqueued) => Task::batch([task, self.toast_unqueued(unqueued)]),
            None => task,
        }
    }

    /// The toast's Retry: queues the files again, and says so again if that
    /// fails too.
    pub(super) fn retry_upload(&mut self, unqueued: Unqueued) -> Task<Message> {
        let Some(store) = self.store.as_ref() else {
            return Task::none();
        };
        match requeue(store, unqueued) {
            Ok(()) => Task::none(),
            Err(unqueued) => self.toast_unqueued(unqueued),
        }
    }

    /// Re-reads the shared account store, and the provider manifests its
    /// rows are described from, before the Accounts page shows it.
    ///
    /// The handle lives as long as the window, and Slate, Envelope and every
    /// sync pass write the same `accounts.toml`; a list from start-up would
    /// show accounts removed elsewhere and miss ones added there.
    pub(super) fn reload_accounts(&mut self) -> Task<Message> {
        self.registry = cosmic_pim_accounts::Registry::load();
        match self.accounts.as_mut().map(reload_account_store) {
            Some(Err(why)) => self.toast(why),
            _ => Task::none(),
        }
    }

    /// The shared account list was written — by the Accounts window, by
    /// Slate or Envelope, or by this window: reads it again, and fetches the
    /// contacts of an account that was not in it before.
    ///
    /// The same reason an account added here is synced at once: someone has
    /// just said where their contacts are. An account added here is already
    /// in the list this reads against, so it is not fetched twice. A list
    /// that cannot be read is logged and not toasted, unlike
    /// [`Self::reload_accounts`]: nobody asked for this read.
    pub(super) fn accounts_file_changed(&mut self) -> Task<Message> {
        let known: Vec<String> = self
            .accounts
            .iter()
            .flat_map(cosmic_pim_accounts::AccountStore::accounts)
            .map(|account| account.id.clone())
            .collect();
        match self.accounts.as_mut() {
            Some(accounts) => {
                if let Err(why) = accounts.reload() {
                    // The copy we have is still better than none.
                    tracing::warn!(%why, "cannot re-read the account store");
                }
            }
            // The store could not be opened at start-up. It may open now.
            None => match cosmic_pim_accounts::AccountStore::open_default() {
                Ok(accounts) => self.accounts = Some(accounts),
                Err(why) => tracing::warn!(%why, "still cannot open the account store"),
            },
        }
        let now = self.accounts.as_ref().map_or(&[][..], |a| a.accounts());
        if has_new_account(&known, now) {
            return self.sync_now();
        }
        Task::none()
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

/// What Add account does: starts the desktop's Accounts window, which takes
/// an address and works out the rest. Where `program` cannot be started —
/// the Accounts window is a package of its own, and may not be installed —
/// the server form here opens instead, which does the job for a CardDAV
/// account.
pub(super) fn add_account_elsewhere(form: &mut Option<AccountForm>, program: &str) {
    if let Err(why) = crate::handoff::start(crate::handoff::add_account(program)) {
        tracing::info!(%why, "no Accounts window; using the server form");
        *form = Some(AccountForm::default());
    }
}

/// Stores the account the server form describes, once the server at its
/// address has taken the login: `setup::add_with_dav_url` finds the principal
/// and the calendar home — or, on a contacts-only server, the address book
/// home — with the password before anything is written. A password stored
/// untried failed later as a sync error, far from the form. Blocking.
///
/// Returns the new account's id.
fn add_server(
    store: &mut cosmic_pim_accounts::AccountStore,
    registry: &cosmic_pim_accounts::Registry,
    form: &AccountForm,
) -> Result<String, ServerNotAdded> {
    use cosmic_pim_sync::setup::{self, SetupError};

    let login = form.username.trim();
    let name = match form.display_name.trim() {
        "" => login,
        name => name,
    };
    // A plan starts from an email address, and this form has a login and a
    // server instead. The plan's address is only a fallback for the name and
    // the login, and both are passed, so it is never what is stored: the
    // login when it is an address, else the login at a reserved domain.
    let plan = setup::plan(registry, login, false)
        .or_else(|| setup::plan(registry, &format!("{login}@server.invalid"), false))
        .ok_or_else(|| ServerNotAdded::Failed(fl!("error-no-username")))?;

    setup::add_with_dav_url(store, &plan, name, &form.url, login, &form.password).map_err(|why| {
        match why {
            SetupError::Rejected(_) => ServerNotAdded::Refused,
            why => ServerNotAdded::Failed(why.to_string()),
        }
    })
}

/// Takes the server's answer into the form: closed when the account was
/// stored, else kept as typed with what went wrong. Whether it was stored.
pub(super) fn server_added(
    form: &mut Option<AccountForm>,
    result: Result<(), ServerNotAdded>,
) -> bool {
    match result {
        Ok(()) => {
            *form = None;
            true
        }
        Err(why) => {
            if let Some(form) = form.as_mut() {
                form.adding = false;
                form.error = Some(match why {
                    ServerNotAdded::Refused => fl!("error-server-refused"),
                    ServerNotAdded::Failed(reason) => {
                        fl!("error-server-not-added", reason = reason)
                    }
                });
            }
            false
        }
    }
}

/// Whether `accounts` holds one whose id is not among `known`.
fn has_new_account(known: &[String], accounts: &[cosmic_pim_accounts::Account]) -> bool {
    accounts.iter().any(|account| !known.contains(&account.id))
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
/// back as an [`Unqueued`] for a toast, because nobody would otherwise know
/// the edit will not reach the server. A local-only book, or a write that
/// left its files as they were, queues nothing and is `Ok(())`.
///
/// `write` must not queue anything itself: the lock is held around it.
pub(super) fn write_and_queue<T>(
    root: &Path,
    book_id: &str,
    file_names: &[&str],
    write: impl FnOnce() -> Result<T, StoreError>,
) -> Result<(T, Result<(), Unqueued>), cosmic_pim_sync::Error> {
    write_and_queue_creating(root, book_id, file_names, || {
        write().map(|value| (value, Vec::new()))
    })
}

/// [`write_and_queue`], for a write that also creates files whose names it
/// picks as it writes — a new group card, the new cards of an import.
///
/// `write` returns its value and the names of the files it created, and
/// each of those is queued with the rest, under the same lock
/// (`cosmic_pim_sync::save_and_queue_creating`). `file_names` are the files
/// that may exist already.
pub(super) fn write_and_queue_creating<T>(
    root: &Path,
    book_id: &str,
    file_names: &[&str],
    write: impl FnOnce() -> Result<(T, Vec<String>), StoreError>,
) -> Result<(T, Result<(), Unqueued>), cosmic_pim_sync::Error> {
    let mut touched: Vec<String> = file_names.iter().map(|&name| name.to_owned()).collect();
    let saved = cosmic_pim_sync::save_and_queue_creating(root, book_id, file_names, || {
        let (value, created) = write()?;
        for name in &created {
            if !touched.contains(name) {
                touched.push(name.clone());
            }
        }
        Ok::<_, StoreError>((value, created))
    })?;
    let queued = saved
        .queued
        .map(|_| ())
        .map_err(|why| Unqueued::new(book_id, touched, &why));
    Ok((saved.value, queued))
}

/// Changes saved on this device that did not get into their book's upload
/// queue, and the first reason why.
///
/// The server keeps its copy of each file until it is queued: by the
/// toast's Retry ([`requeue`]), or by a later write that changes it. Saving
/// the same bytes again queues nothing, because `save_and_queue` skips a
/// file a write left as it was.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unqueued {
    /// `(book id, file name)`, each once.
    files: Vec<(String, String)>,
    why: String,
}

impl Unqueued {
    fn new(book_id: &str, files: Vec<String>, why: &dyn std::fmt::Display) -> Self {
        Self {
            files: files
                .into_iter()
                .map(|file| (book_id.to_owned(), file))
                .collect(),
            why: why.to_string(),
        }
    }

    /// Takes `other`'s files in too, keeping this one's reason.
    fn merge(&mut self, other: Self) {
        for file in other.files {
            if !self.files.contains(&file) {
                self.files.push(file);
            }
        }
    }
}

impl std::fmt::Display for Unqueued {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&fl!("error-queue-upload", why = self.why.as_str()))
    }
}

/// Keeps a queue failure for one toast: the first reason, and every file.
pub(super) fn keep_unqueued(slot: &mut Option<Unqueued>, queued: Result<(), Unqueued>) {
    let Err(unqueued) = queued else {
        return;
    };
    match slot {
        Some(first) => first.merge(unqueued),
        None => *slot = Some(unqueued),
    }
}

/// Queues each of `unqueued`'s files for upload as it is now: its bytes if
/// it is there, its deletion if it is gone. The explicit re-queue
/// (`cosmic_pim_sync::queue_save`, `queue_delete`) for a change whose
/// enqueue failed. What fails again comes back.
pub(super) fn requeue(store: &ContactStore, unqueued: Unqueued) -> Result<(), Unqueued> {
    let mut failed = None;
    for (book_id, file) in unqueued.files {
        let exists = store
            .book(&book_id)
            .is_some_and(|meta| meta.path.join(&file).exists());
        let outcome = if exists {
            cosmic_pim_sync::queue_save(store.root(), &book_id, &file)
        } else {
            cosmic_pim_sync::queue_delete(store.root(), &book_id, &file)
        };
        if let Err(why) = outcome {
            keep_unqueued(&mut failed, Err(Unqueued::new(&book_id, vec![file], &why)));
        }
    }
    failed.map_or(Ok(()), Err)
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
    /// queued is refused before it is made, and so is a write that creates
    /// its file. The helpers used to log that and carry on, so nobody knew
    /// the edit would never reach the server.
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

        let refused = write_and_queue_creating(store.root(), &book.id, &[], || {
            write_contact_raw(&book, "new.vcf", ONE).map(|()| ((), vec!["new.vcf".to_owned()]))
        });
        assert!(refused.is_err());
        assert!(
            !book.path.join("new.vcf").exists(),
            "the created card was written with nothing to queue it"
        );
    }

    /// A synced book holding Ada's card as the server last sent it.
    fn synced_book_with_ada(
        dir: &std::path::Path,
    ) -> (ContactStore, cosmic_pim_core::model::CalendarMeta) {
        use cosmic_pim_caldav::{CalDavStore as _, RemoteEvent, VdirStore};
        let mut store = ContactStore::open(dir).unwrap();
        let book = store
            .create_book("Synced", cosmic_pim_core::model::Rgb(1, 2, 3))
            .unwrap();
        let mut vdir = VdirStore::open_carddav(book.clone()).unwrap();
        vdir.set_remote("/dav/ab/", false).unwrap();
        vdir.upsert(&RemoteEvent {
            href: "/dav/ab/ada.vcf".into(),
            etag: "\"v1\"".into(),
            ics: "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada\r\nEND:VCARD\r\n".into(),
        })
        .unwrap();
        store.refresh();
        (store, book)
    }

    fn pending(
        book: &cosmic_pim_core::model::CalendarMeta,
    ) -> Vec<cosmic_pim_caldav::push::PushOp> {
        use cosmic_pim_caldav::push::PushQueue as _;
        cosmic_pim_caldav::VdirStore::open_carddav(book.clone())
            .unwrap()
            .pending()
            .unwrap()
            .into_iter()
            .map(|push| push.op)
            .collect()
    }

    /// A group card, or a card an import adds, is named as it is written.
    /// It was queued in a second step, after the write and outside the
    /// book's lock; it is written and queued inside it now.
    #[test]
    fn a_created_card_is_written_and_queued_inside_the_books_lock() {
        use cosmic_pim_caldav::push::PushOp;
        let dir = tempfile::tempdir().unwrap();
        let (mut store, book) = synced_book_with_ada(dir.path());
        let root = store.root().to_path_buf();
        let held = |book: &cosmic_pim_core::model::CalendarMeta| {
            std::fs::read_dir(&book.path)
                .unwrap()
                .flatten()
                .filter(|entry| entry.file_name().to_string_lossy().ends_with(".lock"))
                .any(|entry| {
                    matches!(
                        std::fs::File::open(entry.path()).unwrap().try_lock(),
                        Err(std::fs::TryLockError::WouldBlock)
                    )
                })
        };

        let mut locked = false;
        let (group, queued) = write_and_queue_creating(&root, &book.id, &[], || {
            locked = held(&book);
            store
                .create_group(
                    "Friends",
                    &book.id,
                    cosmic_pim_core::vcard::WriteVersion::V4,
                )
                .map(|group| {
                    let file = group.file_name.clone();
                    (group, vec![file])
                })
        })
        .unwrap();
        queued.unwrap();
        assert!(locked, "the group card was written outside the book's lock");
        assert!(
            matches!(&pending(&book)[..], [PushOp::Put { file, .. }] if *file == group.file_name),
            "{:?}",
            pending(&book)
        );
    }

    /// The toast used to say a change that failed to queue would wait "until
    /// this contact is saved again". Saving the same bytes again queues
    /// nothing now; Retry is what queues it, as it is on disk — an upload for
    /// a card that is there, a deletion for one that is gone.
    #[test]
    fn a_change_that_was_not_queued_is_queued_by_retry_not_by_saving_it_again() {
        use cosmic_pim_caldav::push::PushOp;
        use cosmic_pim_core::store::contacts::write_contact_raw;
        const EDITED: &str =
            "BEGIN:VCARD\r\nVERSION:3.0\r\nUID:ada\r\nFN:Ada Lovelace\r\nEND:VCARD\r\n";
        let dir = tempfile::tempdir().unwrap();
        let (store, book) = synced_book_with_ada(dir.path());
        // The edit is on disk; its enqueue failed.
        write_contact_raw(&book, "ada.vcf", EDITED).unwrap();

        let ((), queued) = write_and_queue(store.root(), &book.id, &["ada.vcf"], || {
            write_contact_raw(&book, "ada.vcf", EDITED)
        })
        .unwrap();
        queued.unwrap();
        assert!(
            pending(&book).is_empty(),
            "the same bytes were queued again"
        );

        let unqueued = Unqueued::new(&book.id, vec!["ada.vcf".to_owned()], &"disk full");
        assert!(unqueued.to_string().contains("disk full"));
        requeue(&store, unqueued).unwrap();
        assert!(
            matches!(&pending(&book)[..], [PushOp::Put { file, .. }] if file == "ada.vcf"),
            "{:?}",
            pending(&book)
        );

        std::fs::remove_file(book.path.join("ada.vcf")).unwrap();
        requeue(
            &store,
            Unqueued::new(&book.id, vec!["ada.vcf".to_owned()], &"disk full"),
        )
        .unwrap();
        assert!(
            matches!(&pending(&book)[..], [PushOp::Delete { .. }]),
            "{:?}",
            pending(&book)
        );
    }

    /// One toast for a batch: the first reason, every file once.
    #[test]
    fn queue_failures_gather_into_one() {
        let mut slot = None;
        keep_unqueued(&mut slot, Ok(()));
        assert!(slot.is_none());
        keep_unqueued(
            &mut slot,
            Err(Unqueued::new("a", vec!["x.vcf".into()], &"first")),
        );
        keep_unqueued(
            &mut slot,
            Err(Unqueued::new(
                "a",
                vec!["x.vcf".into(), "y.vcf".into()],
                &"second",
            )),
        );
        let gathered = slot.unwrap();
        assert_eq!(gathered.why, "first");
        assert_eq!(
            gathered.files,
            vec![("a".into(), "x.vcf".into()), ("a".into(), "y.vcf".into())]
        );
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

    /// A WebDAV server on 127.0.0.1 that knows `ada` by `password`, with a
    /// calendar home when `calendars` and an address book home always. One
    /// request per connection.
    fn dav_server(password: &'static str, calendars: bool) -> String {
        use base64::Engine as _;
        use std::io::{BufRead as _, BufReader, Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let port = listener.local_addr().expect("addr").port();
        let expected = format!(
            "Basic {}",
            base64::engine::general_purpose::STANDARD.encode(format!("ada:{password}"))
        );
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(stream) = stream else { return };
                let mut out = stream.try_clone().expect("clone");
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                if reader.read_line(&mut request).is_err() {
                    continue;
                }
                let path = request.split(' ').nth(1).unwrap_or("/").to_owned();
                let (mut length, mut authorized) = (0, false);
                let mut line = String::new();
                while reader.read_line(&mut line).is_ok_and(|n| n > 2) {
                    if let Some((name, value)) = line.split_once(':') {
                        if name.eq_ignore_ascii_case("content-length") {
                            length = value.trim().parse().unwrap_or(0);
                        } else if name.eq_ignore_ascii_case("authorization") {
                            authorized = value.trim() == expected;
                        }
                    }
                    line.clear();
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body);
                let body = String::from_utf8_lossy(&body);

                let _ = if authorized {
                    let prop = if body.contains("current-user-principal") {
                        "<D:current-user-principal><D:href>/principal/</D:href></D:current-user-principal>"
                    } else if body.contains("calendar-home-set") && calendars {
                        "<C:calendar-home-set><D:href>/calendars/</D:href></C:calendar-home-set>"
                    } else if body.contains("addressbook-home-set") {
                        "<C:addressbook-home-set><D:href>/contacts/</D:href></C:addressbook-home-set>"
                    } else {
                        ""
                    };
                    let xml = format!(
                        "<?xml version=\"1.0\"?><D:multistatus xmlns:D=\"DAV:\" \
                         xmlns:C=\"urn:ietf:params:xml:ns:caldav\"><D:response><D:href>{path}</D:href>\
                         <D:propstat><D:prop>{prop}</D:prop><D:status>HTTP/1.1 200 OK</D:status>\
                         </D:propstat></D:response></D:multistatus>"
                    );
                    write!(
                        out,
                        "HTTP/1.1 207 Multi-Status\r\nContent-Type: application/xml; charset=utf-8\r\n\
                         Content-Length: {}\r\nConnection: close\r\n\r\n{xml}",
                        xml.len()
                    )
                } else {
                    write!(
                        out,
                        "HTTP/1.1 401 Unauthorized\r\nWWW-Authenticate: Basic realm=\"dav\"\r\n\
                         Content-Length: 0\r\nConnection: close\r\n\r\n"
                    )
                };
            }
        });
        format!("http://127.0.0.1:{port}/")
    }

    fn private_store(dir: &Path) -> cosmic_pim_accounts::AccountStore {
        let secrets = cosmic_pim_accounts::SecretStore::open_envelope_only("circle-test", dir);
        cosmic_pim_accounts::AccountStore::open(&dir.join("accounts.toml"), secrets)
            .expect("the store")
    }

    fn server_form(url: &str, password: &str) -> AccountForm {
        AccountForm {
            display_name: "Home".into(),
            url: url.into(),
            username: "ada".into(),
            password: password.into(),
            ..AccountForm::default()
        }
    }

    #[test]
    fn a_server_that_takes_the_login_is_stored() {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = private_store(dir.path());
        let registry = cosmic_pim_accounts::Registry::load_from(dir.path());
        let url = dav_server("right", true);

        let id = add_server(&mut store, &registry, &server_form(&url, "right"))
            .expect("the server took the login");

        let account = store.get(&id).expect("stored");
        assert_eq!(account.display_name, "Home");
        assert_eq!(account.url, url);
        assert_eq!(account.username, "ada", "the login, not a made-up address");
        assert_eq!(account.provider, None);
        assert_eq!(store.password(&id).expect("read").as_deref(), Some("right"));
    }

    #[test]
    fn a_contacts_only_server_is_stored_under_the_login_as_its_name() {
        // A CardDAV server with no calendars is the whole point of Circle's
        // form.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = private_store(dir.path());
        let registry = cosmic_pim_accounts::Registry::load_from(dir.path());
        let form = AccountForm {
            display_name: String::new(),
            ..server_form(&dav_server("right", false), "right")
        };

        let id = add_server(&mut store, &registry, &form).expect("accepted");

        assert_eq!(store.get(&id).expect("stored").display_name, "ada");
    }

    #[test]
    fn a_server_that_refuses_the_password_stores_nothing() {
        // It used to be stored as typed, and failed later as a sync error.
        let dir = tempfile::tempdir().expect("tempdir");
        let mut store = private_store(dir.path());
        let registry = cosmic_pim_accounts::Registry::load_from(dir.path());
        let url = dav_server("right", false);

        let refused = add_server(&mut store, &registry, &server_form(&url, "wrong"));

        assert_eq!(refused, Err(ServerNotAdded::Refused));
        assert_eq!(store.accounts(), []);
    }

    #[test]
    fn a_refused_login_keeps_the_form_open_and_says_so() {
        let mut form = Some(AccountForm {
            adding: true,
            ..server_form("https://dav.example/", "wrong")
        });

        assert!(!server_added(&mut form, Err(ServerNotAdded::Refused)));

        let open = form.as_ref().expect("the form stays open");
        assert!(!open.adding);
        assert_eq!(open.password, "wrong", "what was typed is kept to correct");
        assert_eq!(
            open.error.as_deref(),
            Some(fl!("error-server-refused").as_str())
        );

        assert!(!server_added(
            &mut form,
            Err(ServerNotAdded::Failed("connection refused".into()))
        ));
        let error = form
            .as_ref()
            .and_then(|f| f.error.as_deref())
            .unwrap_or_default();
        assert!(error.contains("connection refused"), "{error}");

        assert!(server_added(&mut form, Ok(())));
        assert!(form.is_none(), "an added account closes the form");
    }

    #[test]
    fn only_an_account_that_was_not_in_the_list_is_one_to_fetch() {
        let account = |id: &str| {
            let mut account =
                cosmic_pim_accounts::Account::new(id, "https://dav.example", "ada@example.com");
            account.id = id.to_owned();
            account
        };
        let known = ["a".to_owned()];

        // Added in the Accounts window.
        assert!(has_new_account(&known, &[account("a"), account("b")]));
        assert!(has_new_account(&[], &[account("a")]));

        // Rewritten with the same accounts, or with one removed: nothing to
        // fetch.
        assert!(!has_new_account(&known, &[account("a")]));
        assert!(!has_new_account(&known, &[]));
        assert!(!has_new_account(&[], &[]));
    }

    #[test]
    fn without_the_accounts_window_add_account_opens_the_server_form() {
        // A desktop without the Accounts window installed must still be able
        // to add an account.
        let mut form = None;

        add_account_elsewhere(&mut form, "/nonexistent/magnetar-accounts");

        assert!(form.is_some(), "nothing opened to add an account with");
    }

    #[test]
    fn with_the_accounts_window_the_server_form_stays_closed() {
        // `true` stands in for the Accounts window: it starts.
        let mut form = None;

        add_account_elsewhere(&mut form, "true");

        assert!(form.is_none());
    }
}
