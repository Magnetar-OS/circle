// SPDX-License-Identifier: GPL-3.0-only

//! The Circle application shell.

use cosmic::app::{Core, Task, context_drawer};
use cosmic::cosmic_config::CosmicConfigEntry as _;
use cosmic::iced::Subscription;
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::prelude::*;
use cosmic::widget::menu::action::MenuAction as _;
use cosmic::widget::{self, about::About, menu, nav_bar};
use cosmic_pim_core::model::{CalendarMeta, Contact};
use cosmic_pim_core::store::contacts::ContactStore;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::config::Config;
use crate::fl;
use crate::ui::csv;
use crate::ui::dialogs::Dialog;
use crate::ui::editor;

mod delete;
mod list;
mod save;
mod sync;
mod transfer;
mod view;

pub use sync::SyncSummary;

use delete::card_segment;
use sync::{Unqueued, write_and_queue, write_and_queue_creating};
use transfer::file_label;

const APP_ID: &str = "com.magnetaros.Circle";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
// The file this names must be *committed*, not merely present. Pointing it at
// an untracked path compiles for whoever has that file and breaks the build
// for a clean checkout, which is what CI and every other machine is.
const APP_ICON: &[u8] =
    include_bytes!("../resources/icons/hicolor/scalable/apps/com.magnetaros.Circle.svg");

/// Identifies one contact.
///
/// The UID alone will not do. A UID is unique within a collection, not across
/// them, and the same person synced from two accounts legitimately carries the
/// same UID in both books — which is the normal case Circle's linking model
/// (03, tier 5) is eventually built on, not an error. Keying the selection on
/// the UID alone would make those two rows the same row.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct ContactKey {
    pub book: String,
    pub uid: String,
}

impl ContactKey {
    #[must_use]
    pub fn of(contact: &Contact) -> Self {
        Self {
            book: contact.addressbook_id.clone(),
            uid: contact.uid.clone(),
        }
    }

    #[must_use]
    pub fn matches(&self, contact: &Contact) -> bool {
        self.book == contact.addressbook_id && self.uid == contact.uid
    }
}

/// What a nav-bar entry filters the list to.
#[derive(Clone, Debug, Eq, PartialEq)]
enum NavEntry {
    All,
    Book(String),
    /// Everybody past the cadence they were given — the keep-in-touch smart
    /// list (03 §7). Computed when the list is drawn rather than stored,
    /// because "overdue" is a question about today, not a property of a card.
    Overdue,
    /// A `CATEGORIES` value — the vCard-native half of groups (03, tier 4).
    Category(String),
    /// A `KIND:group` / `X-ADDRESSBOOKSERVER-KIND:group` card — the other
    /// half. Filtering resolves its member URIs against contact UIDs.
    Group {
        book: String,
        uid: String,
    },
}

/// Start-up options, from the command line or a D-Bus activation.
///
/// Build one with [`Flags::new`]: the hand-over request a second launch sends
/// over the bus is derived from the fields once, at construction, because
/// [`cosmic::app::CosmicFlags::action`] can only return a reference to
/// something the struct already owns. Mirrors Slate's `Flags` — the suite's
/// apps should be launched the same way.
#[derive(Clone, Debug, Default)]
pub struct Flags {
    /// `.vcf` files to import on start-up.
    pub import: Vec<PathBuf>,
    /// Open the editor on a blank contact once the window is up.
    pub new_contact: bool,
    /// Start with the list filtered to this query — how the launcher plugin
    /// opens a specific person.
    pub search: Option<String>,
    /// What to ask an already-running instance to do, or `None` when there is
    /// nothing to say and raising its window is the whole request.
    task: Option<CircleTask>,
}

impl Flags {
    #[must_use]
    pub fn new(import: Vec<PathBuf>, new_contact: bool, search: Option<String>) -> Self {
        let task = (new_contact || search.is_some() || !import.is_empty()).then_some(CircleTask {
            new_contact,
            search: search.clone(),
        });
        Self {
            import,
            new_contact,
            search,
            task,
        }
    }
}

/// What a second launch asks the running instance to do. Serialised as JSON
/// across the bus — see Slate's `SlateTask` for why a struct rather than a
/// bespoke encoding. The `.vcf` paths travel in `CosmicFlags::args`, the only
/// part of the request that is a list.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct CircleTask {
    pub new_contact: bool,
    #[serde(default)]
    pub search: Option<String>,
}

impl std::fmt::Display for CircleTask {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned()))
    }
}

impl std::str::FromStr for CircleTask {
    type Err = serde_json::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        serde_json::from_str(s)
    }
}

impl cosmic::app::CosmicFlags for Flags {
    type SubCommand = CircleTask;
    type Args = Vec<String>;

    fn action(&self) -> Option<&Self::SubCommand> {
        self.task.as_ref()
    }

    fn args(&self) -> Vec<&str> {
        self.import
            .iter()
            .filter_map(|path| path.to_str())
            .collect()
    }
}

pub struct AppModel {
    core: Core,
    about: About,
    context_page: ContextPage,
    key_binds: HashMap<menu::KeyBind, MenuAction>,

    config: Config,
    /// The handle writes go through. `None` when cosmic-config is unavailable,
    /// in which case settings still work for the session but do not persist —
    /// a degraded app is better than one that will not start.
    config_handler: Option<cosmic::cosmic_config::Config>,

    store: Option<ContactStore>,
    /// Which cards are the same person. Circle's own metadata, beside the
    /// books rather than inside them — see [`crate::links`].
    links: crate::links::LinkStore,
    /// The non-head cards of every linked person currently in the list, keyed
    /// by the row that stands for them. Rebuilt by `reload`, because which
    /// card is the head depends on which of them the filter left visible.
    folded: HashMap<ContactKey, Vec<Contact>>,
    /// Where the address books live. Held because the link store, the CRM
    /// records and the attachment blobs all sit beside them, and a sandboxed
    /// run must keep its own copies of all three in its own sandbox.
    contacts_root: PathBuf,
    /// Notes, interactions, and cadences — Circle's own data, beside the
    /// books rather than in them. See [`crate::crm`].
    crm: crate::crm::CrmStore,
    /// The note being typed in the detail pane, if any.
    note_draft: String,
    /// The selected person's relationships, resolved against the address book.
    ///
    /// Held rather than derived in `view` because resolving reads every
    /// contact: once per selection is cheap, once per frame is not.
    relations: Vec<crate::relations::Relation>,
    /// `Some` while the duplicate review screen is up. Mutually exclusive
    /// with the editor and the CSV mapper — all three claim the right-hand
    /// pane.
    review: Option<Review>,
    /// Paired, reachable phones, found once at start-up. Empty when KDE
    /// Connect is not installed or nothing is in range, which is the normal
    /// case — the SMS action simply is not offered.
    phones: Vec<crate::kdeconnect::Device>,
    /// Whether an installed application opens `geo:` URIs, found once at
    /// start-up. Without one, addresses offer no "Show on the map".
    can_map: bool,
    /// Accounts and credentials, shared with Slate — one `accounts.toml` for
    /// the whole suite. `None` when the account store could not be opened; the
    /// address book still works, it just cannot sync.
    accounts: Option<cosmic_pim_accounts::AccountStore>,
    /// The account list's size and modification time when it was last read,
    /// so an account added in another application is noticed.
    accounts_stamp: sync::Stamp,
    /// The in-progress "add an account" form on the Accounts page.
    account_form: Option<AccountForm>,
    /// A sync pass is in flight. One at a time: two passes racing on the same
    /// sidecar files is the bug this flag exists to prevent.
    syncing: bool,
    /// The last pass's per-account summaries, shown on the Accounts page.
    sync_status: Option<String>,
    /// The accounts the last pass flagged for attention. A toast is raised
    /// only when this set changes, so a server that stays down does not
    /// interrupt every background pass.
    sync_attention: Vec<String>,
    /// Cards this device and the server both changed, awaiting a decision —
    /// see [`crate::conflicts`]. Re-read from the sync sidecars at start-up,
    /// after every pass, and after every resolution.
    conflicts: Vec<crate::conflicts::ConflictRow>,
    nav: nav_bar::Model,
    /// Writable books, cached as parallel id/name vectors.
    ///
    /// `widget::dropdown` borrows its labels for the lifetime of the view, so
    /// they cannot be built inside `settings_view`; caching them here also
    /// keeps the list off the per-frame path. Rebuilt by `rebuild_nav`.
    writable_ids: Vec<String>,
    writable_names: Vec<String>,

    /// Everything matching the current query and book filter, sorted, the
    /// starred people first.
    contacts: Vec<Contact>,
    /// How many rows at the head of `contacts` are starred — where the
    /// list's "Favorites" section ends. Set by `reload` with the order.
    favorites: usize,
    selected: Option<ContactKey>,
    /// Selection mode: rows toggle membership in `checked` instead of opening
    /// the detail pane, and the action bar under the list operates on the set.
    selecting: bool,
    /// The rows ticked while `selecting`.
    checked: std::collections::HashSet<ContactKey>,
    /// The keyboard modifiers as of the last change — what turns a plain
    /// click into Ctrl+click (toggle) or Shift+click (range).
    modifiers: Modifiers,
    /// Deleted cards whose undo toast is still up, newest last, capped so a
    /// long session cannot hoard every card ever deleted.
    undo: std::collections::BTreeMap<u64, Vec<DeletedCard>>,
    undo_seq: u64,
    /// The window's width, for the adaptive layout. Starts at the configured
    /// launch width; `on_window_resize` keeps it true from then on.
    width: f32,
    /// Decoded photos, one entry per contact that has one.
    ///
    /// Cached because `view` runs every frame and a PHOTO is hundreds of
    /// kilobytes of base64 — decoding per redraw would burn a visible amount
    /// of CPU on an idle window, and re-creating a `Handle` per frame would
    /// defeat the renderer's texture cache too. Filled lazily by `reload` for
    /// the rows in view, and cleared whenever the bytes on disk may have
    /// changed (an external edit, a sync pass, an explicit refresh).
    photos: HashMap<ContactKey, widget::image::Handle>,
    query: String,

    /// `Some` while a contact is being edited or created.
    editor: Option<editor::State>,
    /// `Some` while a CSV mapping screen is up. Mutually exclusive with the
    /// editor — both claim the right-hand pane.
    csv: Option<csv::State>,
    /// `Some` while a confirmation dialog is up.
    dialog: Option<Dialog>,

    toasts: widget::Toasts<Message>,
    /// Set when the store could not be opened at all.
    fatal: Option<String>,
}

/// The duplicate review screen's state: the pairs still to ask about, and a
/// snapshot of the cards they name.
///
/// Snapshotted rather than re-read per frame because the screen compares two
/// specific cards and the comparison must not shift underneath the person
/// reading it. A card deleted while the screen is open simply stops being
/// drawn — see `AppModel::side`.
#[derive(Debug)]
struct Review {
    candidates: Vec<crate::dedupe::Candidate>,
    cards: Vec<Contact>,
}

/// Everything needed to put a deleted card back, byte for byte.
#[derive(Clone, Debug)]
struct DeletedCard {
    book: String,
    uid: String,
    file_name: String,
    raw: String,
    /// The card's notes, interactions and cadence, carried through the
    /// deletion.
    ///
    /// Notes about somebody who no longer exists must not linger — but the
    /// delete is undoable, and an undo that brought back the card and not
    /// what you had written about them would be a quieter kind of loss than
    /// the one it was meant to prevent.
    crm: Option<crate::crm::Record>,
}

/// Below this width the three panes collapse to one — list or detail, not
/// both. Chosen so the collapsed layout arrives well before the 360 px the
/// metainfo promises, with the panes still comfortable either side of it.
const COLLAPSE_WIDTH: f32 = 640.0;

/// The most undo toasts worth honouring at once; older deletions fall off.
const UNDO_DEPTH: usize = 8;

/// The in-progress "add an account" form on the Accounts page.
#[derive(Clone, Debug, Default)]
pub struct AccountForm {
    pub display_name: String,
    pub url: String,
    pub username: String,
    pub password: String,
    pub error: Option<String>,
}

#[derive(Clone, Debug)]
pub enum Message {
    LaunchUrl(String),
    ToggleContextPage(ContextPage),
    UpdateConfig(Config),

    QueryChanged(String),
    /// A search handed over from outside — the launcher plugin, or a second
    /// `circle --search=`. Unlike typing, this selects the person when the
    /// query names exactly one, because the launcher's promise is that Enter
    /// opens *them*, not that it narrows a list they then have to click.
    SearchHandover(String),
    Select(ContactKey),
    SelectFirst,
    MoveSelection(isize),
    Modifiers(Modifiers),
    ToggleSelecting,
    SelectAll,
    BackToList,
    Copy(String),
    FilesChanged,
    Refresh,
    Key(Modifiers, Key, cosmic::iced::keyboard::key::Physical),
    FocusSearch,
    CloseToast(widget::ToastId),
    UndoDelete(u64),
    /// A queue-failure toast's Retry: queue these saved changes for upload
    /// again.
    RetryUpload(Unqueued),

    ToggleBook(String),
    SortByGivenName(bool),
    DefaultBook(usize),
    PreferVcard4(bool),
    SyncInterval(usize),

    /// Open the desktop's Accounts window, where an account is added for the
    /// whole suite by its address.
    OpenAccountsWindow,
    /// Time to look whether the shared account list changed on disk.
    AccountsFileCheck,
    AccountAddStart,
    AccountAddCancel,
    AccountAddConfirm,
    AccountNameChanged(String),
    AccountUrlChanged(String),
    AccountUsernameChanged(String),
    AccountPasswordChanged(String),
    AccountRemove(String),
    SyncNow,
    SyncFinished(SyncSummary),
    /// Open the Accounts page — the sync-problem toast's action.
    ShowAccounts,
    /// Answer the conflict at this index in `conflicts`.
    ConflictResolve(usize, crate::conflicts::Resolution),
    /// (conflict index, disputed-property index, side).
    ConflictChoose(usize, usize, cosmic_pim_core::merge::Side),

    NewContact,
    EditContact,
    Editor(editor::Message),
    EditorSave,
    EditorCancel,

    ImportRequested,
    ImportPath(PathBuf),
    ImportCsvRequested,
    ImportCsvPath(PathBuf),
    Csv(csv::Message),
    CsvConfirm,
    CsvCancel,
    ExportRequested,
    ExportTo(PathBuf, Vec<String>),
    DialogCancelled,
    DialogFailed(String),

    DeleteRequested,
    DeleteConfirmed,
    DialogCancel,

    Unlink(ContactKey),
    ShareRequested,
    /// Star or unstar the person on this row — the list's context menu and
    /// the star beside the name.
    SetFavorite(ContactKey, bool),
    /// The same for whoever is selected — the Edit menu and its shortcut.
    ToggleFavorite,

    LogInteraction,
    SetCadence(usize),
    AttachRequested,
    AttachPath(PathBuf),
    OpenAttachment(String),
    DetachFile(String),
    NoteInput(String),
    AddNote,
    RemoveNote(String),
    PhonesFound(Vec<crate::kdeconnect::Device>),
    MapsFound(bool),
    SmsRequested(String),
    SmsBody(String),
    SmsSend,
    SmsSent(Option<String>),
    LinkChecked,
    ReviewDuplicates,
    ReviewLink(usize),
    ReviewIgnore(usize),
    ReviewClose,

    ExportSelectedRequested,
    ExportSelectedTo(PathBuf),
    AddToGroupRequested,
    AddToGroupName(String),
    AddToGroupConfirmed,

    NewGroupRequested,
    NewGroupName(String),
    NewGroupConfirmed,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum ContextPage {
    #[default]
    About,
    Settings,
    Accounts,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MenuAction {
    NewContact,
    NewGroup,
    EditContact,
    Delete,
    SelectAll,
    Duplicates,
    Share,
    Favorite,
    Search,
    Import,
    ImportCsv,
    Export,
    Refresh,
    SyncNow,
    Accounts,
    Settings,
    About,
}

impl menu::action::MenuAction for MenuAction {
    type Message = Message;

    fn message(&self) -> Self::Message {
        match self {
            MenuAction::NewContact => Message::NewContact,
            MenuAction::NewGroup => Message::NewGroupRequested,
            MenuAction::EditContact => Message::EditContact,
            MenuAction::Delete => Message::DeleteRequested,
            MenuAction::SelectAll => Message::SelectAll,
            MenuAction::Duplicates => Message::ReviewDuplicates,
            MenuAction::Share => Message::ShareRequested,
            MenuAction::Favorite => Message::ToggleFavorite,
            MenuAction::Search => Message::FocusSearch,
            MenuAction::Import => Message::ImportRequested,
            MenuAction::ImportCsv => Message::ImportCsvRequested,
            MenuAction::Export => Message::ExportRequested,
            MenuAction::Refresh => Message::Refresh,
            MenuAction::SyncNow => Message::SyncNow,
            MenuAction::Accounts => Message::ToggleContextPage(ContextPage::Accounts),
            MenuAction::Settings => Message::ToggleContextPage(ContextPage::Settings),
            MenuAction::About => Message::ToggleContextPage(ContextPage::About),
        }
    }
}

/// The search field's id, so `Ctrl+F` has something to focus.
fn search_id() -> widget::Id {
    widget::Id::new("search")
}

impl cosmic::Application for AppModel {
    type Executor = cosmic::executor::Default;
    type Flags = Flags;
    type Message = Message;
    const APP_ID: &'static str = APP_ID;

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let about = About::default()
            .name(fl!("app-title"))
            .icon(widget::icon::from_svg_bytes(APP_ICON))
            .version(env!("CARGO_PKG_VERSION"))
            .license(env!("CARGO_PKG_LICENSE"))
            .links([(fl!("repository"), REPOSITORY)]);

        let config_handler = cosmic::cosmic_config::Config::new(APP_ID, Config::VERSION).ok();
        let config = config_handler
            .as_ref()
            .map(|handler| match Config::get_entry(handler) {
                Ok(config) => config,
                Err((errors, config)) => {
                    for why in errors {
                        tracing::warn!(%why, "error loading the configuration");
                    }
                    config
                }
            })
            .unwrap_or_default();

        let (store, fatal) = match ContactStore::open_default() {
            Ok(store) => (Some(store), None),
            Err(why) => {
                tracing::error!(%why, "cannot open the contact store");
                (None, Some(why.to_string()))
            }
        };
        // The link store lives beside the books, under whichever root the
        // store actually opened — so a sandboxed run links in its sandbox.
        let contacts_root = store
            .as_ref()
            .map_or_else(cosmic_pim_core::store::contacts::default_root, |store| {
                store.root().to_path_buf()
            });

        // Shared with Slate: the same accounts.toml, the same keychain slots.
        // An account added in either app syncs for both.
        let accounts = match cosmic_pim_accounts::AccountStore::open_default() {
            Ok(accounts) => Some(accounts),
            Err(why) => {
                tracing::warn!(%why, "cannot open the account store; sync is unavailable");
                None
            }
        };

        let mut model = Self {
            core,
            about,
            context_page: ContextPage::default(),
            key_binds: crate::key_bind::key_binds(),
            config,
            config_handler,
            store,
            links: crate::links::LinkStore::open(&contacts_root),
            crm: crate::crm::CrmStore::open(&contacts_root),
            contacts_root,
            note_draft: String::new(),
            relations: Vec::new(),
            phones: Vec::new(),
            can_map: false,
            folded: HashMap::new(),
            review: None,
            accounts_stamp: sync::accounts_file_stamp(),
            accounts,
            account_form: None,
            syncing: false,
            sync_status: None,
            sync_attention: Vec::new(),
            conflicts: Vec::new(),
            nav: nav_bar::Model::default(),
            writable_ids: Vec::new(),
            writable_names: Vec::new(),
            contacts: Vec::new(),
            favorites: 0,
            selected: None,
            selecting: false,
            checked: std::collections::HashSet::new(),
            modifiers: Modifiers::default(),
            undo: std::collections::BTreeMap::new(),
            undo_seq: 0,
            width: 1100.0,
            photos: HashMap::new(),
            query: String::new(),
            editor: None,
            csv: None,
            dialog: None,
            toasts: widget::Toasts::new(Message::CloseToast),
            fatal: None,
        };
        model.fatal = fatal;

        // Blobs whose contact was deleted without the delete being undone are
        // orphans. Swept here rather than at delete time, because a delete is
        // undoable and bytes removed then could not come back.
        let swept = crate::attachments::sweep_orphans(&model.contacts_root, &model.crm);
        if swept > 0 {
            tracing::info!(count = swept, "removed orphaned attachments");
        }
        model.rebuild_nav();
        model.reload();
        model.reload_conflicts();

        // Start-up requests from the command line: `.vcf` paths to import, and
        // the desktop entry's "New Contact" action.
        let mut tasks = vec![
            model.update_title(),
            // Ask once whether a phone is in reach. Off the UI thread, and
            // absent-friendly: no daemon means an empty list and no SMS
            // button, not an error.
            cosmic::task::future(async {
                Message::PhonesFound(crate::kdeconnect::devices().await)
            }),
            // The same for a maps application: every desktop entry is read,
            // so not on the UI thread.
            cosmic::task::future(async {
                let found = tokio::task::spawn_blocking(crate::maps::handler_installed).await;
                Message::MapsFound(found.unwrap_or_else(|why| {
                    tracing::warn!(%why, "could not look for a maps application");
                    false
                }))
            }),
        ];
        for path in flags.import {
            tasks.push(cosmic::task::message(cosmic::Action::App(
                Message::ImportPath(path),
            )));
        }
        if let Some(query) = flags.search {
            tasks.push(cosmic::task::message(cosmic::Action::App(
                Message::SearchHandover(query),
            )));
        }
        if flags.new_contact {
            tasks.push(cosmic::task::message(cosmic::Action::App(
                Message::NewContact,
            )));
        }
        (model, Task::batch(tasks))
    }

    /// A second launch handing over its command line, or a launcher invoking
    /// one of the desktop entry's `Actions=`. Without this the desktop entry
    /// and the `MimeType=` registration are promises with nothing behind them.
    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;

        match msg.msg {
            // Plain launch: just raise the window, which the runtime has
            // already done.
            Details::Activate => Task::none(),

            Details::Open { url } => {
                let paths: Vec<PathBuf> = url
                    .iter()
                    .filter_map(|url| url.to_file_path().ok())
                    .collect();
                if paths.is_empty() {
                    tracing::warn!(?url, "activation carried no local files");
                    return Task::none();
                }
                Task::batch(paths.into_iter().map(|path| {
                    cosmic::task::message(cosmic::Action::App(Message::ImportPath(path)))
                }))
            }

            // A launcher sends the bare action name from `Actions=`; a second
            // `circle` process sends a serialised `CircleTask` with the `.vcf`
            // paths in `args`.
            Details::ActivateAction { action, args } => match action.as_str() {
                "new-contact" => cosmic::task::message(cosmic::Action::App(Message::NewContact)),
                encoded => {
                    let Ok(task) = encoded.parse::<CircleTask>() else {
                        tracing::warn!(action = encoded, ?args, "unknown activation action");
                        return Task::none();
                    };
                    let mut tasks: Vec<Task<Self::Message>> = args
                        .iter()
                        .map(|arg| {
                            cosmic::task::message(cosmic::Action::App(Message::ImportPath(
                                PathBuf::from(arg),
                            )))
                        })
                        .collect();
                    if let Some(query) = task.search {
                        tasks.push(cosmic::task::message(cosmic::Action::App(
                            Message::SearchHandover(query),
                        )));
                    }
                    if task.new_contact {
                        tasks.push(cosmic::task::message(cosmic::Action::App(
                            Message::NewContact,
                        )));
                    }
                    Task::batch(tasks)
                }
            },
        }
    }

    /// libcosmic's own keyboard navigation owns Ctrl+F and calls this — the
    /// hook exists so applications do not each listen for the chord themselves.
    fn on_search(&mut self) -> Task<Self::Message> {
        widget::text_input::focus(search_id())
    }

    fn header_start(&self) -> Vec<Element<'_, Self::Message>> {
        crate::ui::menus::bar(
            &self.key_binds,
            self.selected.is_some() && self.editor.is_none(),
            self.selected
                .as_ref()
                .is_some_and(|key| self.is_favorite(key)),
        )
    }

    fn nav_model(&self) -> Option<&nav_bar::Model> {
        // Hidden while editing: switching books mid-edit would either discard
        // the edit or leave the editor pointing at a contact the list no longer
        // shows, and neither is worth the plumbing.
        (self.editor.is_none() && self.fatal.is_none()).then_some(&self.nav)
    }

    fn on_nav_select(&mut self, id: nav_bar::Id) -> Task<Self::Message> {
        self.nav.activate(id);
        self.reload();
        self.update_title()
    }

    fn on_escape(&mut self) -> Task<Self::Message> {
        if self.dialog.is_some() {
            self.dialog = None;
        } else if self.review.is_some() {
            self.review = None;
        } else if self.csv.is_some() {
            self.csv = None;
        } else if self.editor.is_some() {
            self.editor = None;
        } else if self.selecting {
            self.selecting = false;
            self.checked.clear();
        } else if self.is_collapsed() && self.selected.is_some() {
            // In the one-pane layout Escape is "back": detail returns to the
            // list before the query is touched.
            self.selected = None;
        } else if !self.query.is_empty() {
            self.query.clear();
            self.reload();
        }
        Task::none()
    }

    fn on_window_resize(&mut self, _id: cosmic::iced::window::Id, width: f32, _height: f32) {
        self.width = width;
    }

    fn dialog(&self) -> Option<Element<'_, Self::Message>> {
        let dialog = self.dialog.as_ref()?;
        // The two dialogs that need more than their own state get it here:
        // the share code composes a person, and the SMS dialog names the
        // phone it will go through.
        let cards = match dialog {
            Dialog::Share { key } => self.cards_for(key),
            _ => Vec::new(),
        };
        crate::ui::dialogs::view(
            dialog,
            crate::ui::person::compose(&cards).as_ref(),
            self.phones.first().map(|phone| phone.name.as_str()),
            self.checked.len(),
        )
    }

    fn context_drawer(&self) -> Option<context_drawer::ContextDrawer<'_, Self::Message>> {
        if !self.core.window.show_context {
            return None;
        }
        Some(match self.context_page {
            ContextPage::About => context_drawer::about(
                &self.about,
                |url| Message::LaunchUrl(url.to_string()),
                Message::ToggleContextPage(ContextPage::About),
            ),
            ContextPage::Settings => context_drawer::context_drawer(
                crate::ui::settings::view(
                    &self.config,
                    self.store.as_ref().map_or(&[], ContactStore::books),
                    &self.writable_ids,
                    &self.writable_names,
                    self.accounts.is_some(),
                ),
                Message::ToggleContextPage(ContextPage::Settings),
            )
            .title(fl!("settings")),
            ContextPage::Accounts => context_drawer::context_drawer(
                crate::ui::accounts::view(
                    self.accounts.as_ref().map_or(&[], |a| a.accounts()),
                    self.account_form.as_ref(),
                    self.syncing,
                    self.sync_status.as_deref(),
                    &self.conflicts,
                ),
                Message::ToggleContextPage(ContextPage::Accounts),
            )
            .title(fl!("accounts")),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        self.main_view()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subscriptions = vec![
            self.core()
                .watch_config::<Config>(Self::APP_ID)
                .map(|update| {
                    for why in update.errors {
                        tracing::debug!(?why, "config watch error");
                    }
                    Message::UpdateConfig(update.config)
                }),
            file_watch_subscription(),
            // Accounts are the suite's: one added in the Accounts window, or
            // in Envelope or Slate, appears here without a restart. A `stat`
            // every two seconds.
            cosmic::iced::time::every(sync::ACCOUNTS_FILE_CHECK)
                .map(|_| Message::AccountsFileCheck),
            // Only `Ignored` presses: a focused text input has already claimed
            // anything it wants, so the editor keeps its own keys. The physical
            // key travels too — it is what lets Ctrl+N fire on a Greek or
            // Cyrillic layout, where the logical key is not "n". Modifier
            // changes travel regardless of status: what turns a click into
            // Ctrl+click must be current even while a widget has focus.
            cosmic::iced::event::listen_with(|event, status, _window| match (event, status) {
                (
                    cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::KeyPressed {
                        modifiers,
                        key,
                        physical_key,
                        ..
                    }),
                    cosmic::iced::event::Status::Ignored,
                ) => Some(Message::Key(modifiers, key, physical_key)),
                (
                    cosmic::iced::Event::Keyboard(cosmic::iced::keyboard::Event::ModifiersChanged(
                        modifiers,
                    )),
                    _,
                ) => Some(Message::Modifiers(modifiers)),
                _ => None,
            }),
        ];

        // Background sync, on the user's chosen cadence. The tick sends a
        // plain SyncNow, so a pass already in flight makes it a no-op rather
        // than a second concurrent pass.
        if self.config.sync_interval_minutes > 0 && self.accounts.is_some() {
            let minutes = u64::from(self.config.sync_interval_minutes);
            subscriptions.push(
                cosmic::iced::time::every(std::time::Duration::from_secs(minutes * 60))
                    .map(|_| Message::SyncNow),
            );
        }

        Subscription::batch(subscriptions)
    }

    /// Dispatches one message, then re-resolves relationships if the
    /// selection moved.
    ///
    /// The check lives here rather than beside each of the dozen places that
    /// set `selected` — several of which return early — so a new one cannot
    /// forget it. `update` itself stays the single match the conventions ask
    /// for; this only wraps it.
    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        let before = self.selected.clone();
        let task = self.dispatch(message);
        if self.selected != before {
            self.refresh_relations();
        }
        task
    }
}

impl AppModel {
    #[allow(clippy::too_many_lines)]
    fn dispatch(&mut self, message: Message) -> Task<Message> {
        match message {
            Message::LaunchUrl(url) => {
                if let Err(why) = open::that_detached(&url) {
                    return self.toast(format!("{url}: {why}"));
                }
            }
            Message::ToggleContextPage(page) => {
                if self.context_page == page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = page;
                    self.core.window.show_context = true;
                }
                if page == ContextPage::Accounts && self.core.window.show_context {
                    return self.reload_accounts();
                }
            }
            Message::UpdateConfig(config) => {
                self.config = config;
                self.rebuild_nav();
                self.reload();
            }
            Message::QueryChanged(query) => {
                self.query = query;
                self.reload();
            }
            Message::Select(key) => {
                if self.selecting || self.modifiers.control() {
                    self.selecting = true;
                    if self.modifiers.shift() {
                        self.check_range_to(&key);
                    } else if !self.checked.insert(key.clone()) {
                        // Already ticked: a second click unticks.
                        self.checked.remove(&key);
                    }
                } else if self.modifiers.shift() && self.selected.is_some() {
                    self.selecting = true;
                    self.check_range_to(&key);
                } else {
                    self.selected = Some(key);
                    self.refresh_relations();
                }
            }
            Message::SearchHandover(query) => {
                self.query = query;
                self.reload();
                // Exactly one match is unambiguous; two or more is a list the
                // user still has to choose from, and choosing for them would
                // be a guess.
                if let [only] = self.contacts.as_slice() {
                    self.selected = Some(ContactKey::of(only));
                    self.refresh_relations();
                }
            }
            Message::SelectFirst => {
                if let Some(first) = self.contacts.first() {
                    self.selected = Some(ContactKey::of(first));
                }
            }
            Message::MoveSelection(delta) => {
                if self.editor.is_some()
                    || self.csv.is_some()
                    || self.dialog.is_some()
                    || self.contacts.is_empty()
                {
                    return Task::none();
                }
                let position = self
                    .selected
                    .as_ref()
                    .and_then(|key| self.contacts.iter().position(|c| key.matches(c)));
                let next = match position {
                    Some(index) => index
                        .saturating_add_signed(delta)
                        .min(self.contacts.len() - 1),
                    // Nothing selected yet: Down starts at the top, Up at the
                    // bottom, which is where each key is headed anyway.
                    None if delta >= 0 => 0,
                    None => self.contacts.len() - 1,
                };
                self.selected = Some(ContactKey::of(&self.contacts[next]));
            }
            Message::Modifiers(modifiers) => self.modifiers = modifiers,
            Message::ToggleSelecting => {
                self.selecting = !self.selecting;
                if !self.selecting {
                    self.checked.clear();
                }
            }
            Message::SelectAll => {
                self.selecting = true;
                self.checked = self.contacts.iter().map(ContactKey::of).collect();
            }
            Message::BackToList => self.selected = None,
            Message::UndoDelete(token) => return self.undo_delete(token),
            Message::RetryUpload(unqueued) => return self.retry_upload(unqueued),
            Message::Copy(value) => return cosmic::iced::clipboard::write(value),
            // A burst of filesystem changes and an explicit refresh do the same
            // work; they are separate messages only so the logs distinguish
            // "vdirsyncer ran" from "the user pressed Ctrl+R".
            Message::FilesChanged | Message::Refresh => {
                if let Some(store) = self.store.as_mut() {
                    store.refresh();
                }
                // The bytes on disk may have changed under any entry — that is
                // exactly what a sync run editing cards looks like.
                self.photos.clear();
                self.rebuild_nav();
                self.reload();
            }
            Message::Key(modifiers, key, physical) => {
                // Unmodified arrows walk the list. Safe without a modifier —
                // unlike letters, an arrow in a focused text field never
                // reaches here (the widget claims it), so this only fires
                // when nothing is being typed.
                if modifiers.is_empty() {
                    use cosmic::iced::keyboard::key::Named;
                    if matches!(&key, Key::Named(Named::ArrowDown)) {
                        return self.dispatch(Message::MoveSelection(1));
                    }
                    if matches!(&key, Key::Named(Named::ArrowUp)) {
                        return self.dispatch(Message::MoveSelection(-1));
                    }
                }
                for (bind, action) in &self.key_binds {
                    if bind.matches(modifiers, &key, Some(&physical)) {
                        return self.dispatch(action.message());
                    }
                }
            }
            Message::FocusSearch => return widget::text_input::focus(search_id()),
            Message::CloseToast(id) => self.toasts.remove(id),

            Message::ToggleBook(id) => {
                self.config.toggle_book(&id);
                self.persist_config();
                self.rebuild_nav();
                self.reload();
            }
            Message::SortByGivenName(value) => {
                self.config.sort_by_given_name = value;
                self.persist_config();
                self.reload();
            }
            Message::DefaultBook(index) => {
                self.config.default_book = self.writable_ids.get(index).cloned();
                self.persist_config();
            }
            Message::PreferVcard4(value) => {
                self.config.prefer_vcard4 = value;
                self.persist_config();
            }
            Message::SyncInterval(index) => {
                self.config.sync_interval_minutes = crate::ui::settings::SYNC_INTERVALS
                    .get(index)
                    .copied()
                    .unwrap_or_default();
                self.persist_config();
            }

            Message::OpenAccountsWindow => {
                sync::add_account_elsewhere(
                    &mut self.account_form,
                    crate::handoff::ACCOUNTS_WINDOW,
                );
            }
            Message::AccountsFileCheck => {
                let path = cosmic_pim_accounts::account::default_config_path();
                if sync::stamp_changed(&mut self.accounts_stamp, &path) {
                    return self.accounts_file_changed();
                }
            }
            Message::AccountAddStart => self.account_form = Some(AccountForm::default()),
            Message::AccountAddCancel => self.account_form = None,
            Message::AccountAddConfirm => return self.confirm_account(),
            Message::AccountNameChanged(value) => {
                self.with_account_form(|form| form.display_name = value);
            }
            Message::AccountUrlChanged(value) => {
                self.with_account_form(|form| form.url = value);
            }
            Message::AccountUsernameChanged(value) => {
                self.with_account_form(|form| form.username = value);
            }
            Message::AccountPasswordChanged(value) => {
                self.with_account_form(|form| form.password = value);
            }
            Message::AccountRemove(id) => {
                if let Some(accounts) = self.accounts.as_mut()
                    && let Err(why) = accounts.remove(&id)
                {
                    return self.toast(format!("{}: {why}", fl!("accounts")));
                }
            }
            Message::SyncNow => return self.sync_now(),
            Message::ConflictResolve(index, how) => return self.resolve_conflict(index, how),
            Message::ConflictChoose(row, unit, side) => {
                if let Some(disputes) = self
                    .conflicts
                    .get_mut(row)
                    .and_then(|row| row.disputes.as_mut())
                {
                    disputes.choose(unit, side);
                }
            }
            Message::SyncFinished(summary) => {
                self.syncing = false;
                self.sync_status = Some(summary.lines.join("\n"));
                // A pass can record new conflicts without changing a card.
                self.reload_conflicts();
                // The pass re-read accounts.toml on its own handle and may have
                // bound new collections; this one was opened at start-up.
                let reloaded = self.reload_accounts();
                let alert = Task::batch([self.sync_alert(summary.attention), reloaded]);
                if summary.changed {
                    // Sync wrote `.vcf` files directly; everything read from
                    // them — the list, the nav, the photo cache — is stale.
                    if let Some(store) = self.store.as_mut() {
                        store.refresh();
                    }
                    self.photos.clear();
                    self.rebuild_nav();
                    self.reload();
                }
                return alert;
            }
            Message::ShowAccounts => {
                self.context_page = ContextPage::Accounts;
                self.core.window.show_context = true;
                return self.reload_accounts();
            }

            Message::NewContact => {
                // The menu items are disabled while the editor is open, but the
                // key bindings are not routed through the menu — without this
                // guard Ctrl+N mid-edit would replace the editor's state and
                // silently discard whatever had been typed.
                if self.editor.is_some() || self.csv.is_some() || self.review.is_some() {
                    return Task::none();
                }
                let Some(store) = self.store.as_ref() else {
                    return Task::none();
                };
                let books: Vec<CalendarMeta> = store.books().to_vec();
                match self.config.new_card_book(&books) {
                    Some(book) => {
                        let groups = self.group_rows(&book, None);
                        self.editor =
                            Some(editor::State::create(&book, &books).with_groups(groups));
                    }
                    // Every book read-only, or none at all. Saying so is the
                    // whole job here — an editor that cannot save is a trap.
                    None => return self.toast(fl!("error-no-writable-book")),
                }
            }
            Message::EditContact => {
                if self.editor.is_some() || self.csv.is_some() || self.review.is_some() {
                    return Task::none();
                }
                let Some(contact) = self.selected_contact().cloned() else {
                    return Task::none();
                };
                let books: Vec<CalendarMeta> = self
                    .store
                    .as_ref()
                    .map(|s| s.books().to_vec())
                    .unwrap_or_default();

                if books
                    .iter()
                    .any(|b| b.id == contact.addressbook_id && b.read_only)
                {
                    let name = self
                        .store
                        .as_ref()
                        .and_then(|s| s.book(&contact.addressbook_id))
                        .map_or_else(|| contact.addressbook_id.clone(), |b| b.name.clone());
                    return self.toast(fl!("read-only-book", name = name));
                }

                let groups = self.group_rows(&contact.addressbook_id, Some(&contact.uid));
                self.editor = Some(editor::State::edit(contact, &books).with_groups(groups));
            }
            Message::Editor(message) => {
                // The one editor message the shell answers itself: the file
                // dialog is async, and the editor has no runtime to await it.
                if matches!(message, editor::Message::PhotoPickRequested) {
                    return cosmic::task::future(async {
                        use cosmic::dialog::file_chooser::{self, FileFilter};

                        let dialog = file_chooser::open::Dialog::new()
                            .title(fl!("set-photo"))
                            .filter(
                                FileFilter::new(fl!("photo").as_str())
                                    .glob("*.png")
                                    .glob("*.jpg")
                                    .glob("*.jpeg")
                                    .glob("*.webp"),
                            );

                        match dialog.open_file().await {
                            Ok(response) => match response.url().to_file_path() {
                                Ok(path) => Message::Editor(editor::Message::PhotoChosen(path)),
                                Err(()) => Message::DialogFailed(fl!("error-remote-file")),
                            },
                            Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                            Err(why) => Message::DialogFailed(why.to_string()),
                        }
                    });
                }
                let book_changed = matches!(message, editor::Message::Book(_));
                if let Some(state) = self.editor.as_mut() {
                    state.update(message);
                }
                // A new contact moved to another book: offer that book's
                // groups, which the editor cannot read for itself.
                if book_changed
                    && let Some(book) = self
                        .editor
                        .as_ref()
                        .filter(|state| state.groups.is_empty())
                        .map(|state| state.contact.addressbook_id.clone())
                {
                    let groups = self.group_rows(&book, None);
                    if let Some(state) = self.editor.as_mut() {
                        state.groups = groups;
                    }
                }
            }
            Message::EditorSave => return self.save_editor(),
            Message::EditorCancel => self.editor = None,

            Message::ImportRequested => {
                return cosmic::task::future(async {
                    use cosmic::dialog::file_chooser::{self, FileFilter};

                    let dialog = file_chooser::open::Dialog::new()
                        .title(fl!("import"))
                        .filter(FileFilter::new("vCard").glob("*.vcf"));

                    match dialog.open_file().await {
                        Ok(response) => match response.url().to_file_path() {
                            Ok(path) => Message::ImportPath(path),
                            Err(()) => Message::DialogFailed(fl!("error-remote-file")),
                        },
                        Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                        Err(why) => Message::DialogFailed(why.to_string()),
                    }
                });
            }
            Message::ImportPath(path) => return self.import(&path),

            Message::ImportCsvRequested => {
                if self.editor.is_some() {
                    // The editor owns the pane and possibly unsaved text.
                    return Task::none();
                }
                return cosmic::task::future(async {
                    use cosmic::dialog::file_chooser::{self, FileFilter};

                    let dialog = file_chooser::open::Dialog::new()
                        .title(fl!("import-csv"))
                        .filter(FileFilter::new("CSV").glob("*.csv"));

                    match dialog.open_file().await {
                        Ok(response) => match response.url().to_file_path() {
                            Ok(path) => Message::ImportCsvPath(path),
                            Err(()) => Message::DialogFailed(fl!("error-remote-file")),
                        },
                        Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                        Err(why) => Message::DialogFailed(why.to_string()),
                    }
                });
            }
            Message::ImportCsvPath(path) => match csv::State::open(&path) {
                Ok(state) => self.csv = Some(state),
                Err(why) => return self.toast(why),
            },
            Message::Csv(message) => {
                if let Some(state) = self.csv.as_mut() {
                    state.update(message);
                }
            }
            Message::CsvCancel => self.csv = None,
            Message::CsvConfirm => return self.import_csv(),

            Message::ExportRequested => {
                // The active nav filter decides the scope: one book exports
                // under its own name, "All contacts" exports every visible book
                // into one file.
                let (name, ids) = match self.nav.active_data::<NavEntry>() {
                    Some(NavEntry::Book(id)) => {
                        let name = self
                            .store
                            .as_ref()
                            .and_then(|s| s.book(id))
                            .map_or_else(|| id.clone(), |b| b.name.clone());
                        (name, vec![id.clone()])
                    }
                    _ => {
                        let ids: Vec<String> = self
                            .store
                            .as_ref()
                            .map(|s| {
                                s.books()
                                    .iter()
                                    .filter(|b| !self.config.is_hidden(&b.id))
                                    .map(|b| b.id.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                        (fl!("all-contacts"), ids)
                    }
                };
                if ids.is_empty() {
                    // No book exists or every one is hidden: exporting needs
                    // no write access, so the old "no writable book" was
                    // wrong about why.
                    return self.toast(fl!("error-nothing-to-export"));
                }

                return cosmic::task::future(async move {
                    use cosmic::dialog::file_chooser::{self, FileFilter};

                    let dialog = file_chooser::save::Dialog::new()
                        .title(fl!("export"))
                        .file_name(format!("{name}.vcf"))
                        .filter(FileFilter::new("vCard").glob("*.vcf"));

                    match dialog.save_file().await {
                        Ok(response) => match response.url().and_then(|u| u.to_file_path().ok()) {
                            Some(path) => Message::ExportTo(path, ids),
                            None => Message::DialogFailed(fl!("error-remote-file")),
                        },
                        Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                        Err(why) => Message::DialogFailed(why.to_string()),
                    }
                });
            }
            Message::ExportTo(path, book_ids) => {
                let Some(store) = self.store.as_ref() else {
                    return Task::none();
                };
                let mut text = String::new();
                for id in &book_ids {
                    match store.export_book(id) {
                        Ok(part) => text.push_str(&part),
                        Err(why) => return self.toast(why.to_string()),
                    }
                }
                match std::fs::write(&path, text) {
                    Ok(()) => return self.toast(fl!("export-done", path = file_label(&path))),
                    Err(why) => return self.toast(format!("{}: {why}", file_label(&path))),
                }
            }
            Message::DialogCancelled => {}
            Message::DialogFailed(why) => return self.toast(why),

            Message::ShareRequested => {
                if let Some(contact) = self.selected_contact() {
                    self.dialog = Some(Dialog::Share {
                        key: ContactKey::of(contact),
                    });
                }
            }
            Message::SetFavorite(key, favorite) => return self.set_favorite(&key, favorite),
            Message::ToggleFavorite => {
                // Disabled in the menu while the editor is open; the key
                // binding is not routed through the menu, so it asks too.
                if self.editor.is_none()
                    && let Some(key) = self.selected.clone()
                {
                    let favorite = !self.is_favorite(&key);
                    return self.set_favorite(&key, favorite);
                }
            }
            Message::PhonesFound(phones) => {
                if !phones.is_empty() {
                    tracing::info!(count = phones.len(), "KDE Connect devices in reach");
                }
                self.phones = phones;
            }
            Message::MapsFound(found) => self.can_map = found,
            Message::SmsRequested(number) => {
                if self.phones.is_empty() {
                    return Task::none();
                }
                self.dialog = Some(Dialog::Sms {
                    number,
                    body: String::new(),
                    sending: false,
                });
            }
            Message::SmsBody(text) => {
                if let Some(Dialog::Sms { body, .. }) = self.dialog.as_mut() {
                    *body = text;
                }
            }
            Message::SmsSend => {
                let Some(Dialog::Sms {
                    number,
                    body,
                    sending,
                }) = self.dialog.as_mut()
                else {
                    return Task::none();
                };
                if body.trim().is_empty() || *sending {
                    return Task::none();
                }
                // The dialog stays up, disabled, until the daemon answers:
                // closing it on send would leave a failure with nowhere to
                // appear and the typed message gone.
                *sending = true;
                let (number, body) = (number.clone(), body.clone());
                let Some(device) = self.phones.first().map(|phone| phone.id.clone()) else {
                    return Task::none();
                };
                return cosmic::task::future(async move {
                    Message::SmsSent(
                        crate::kdeconnect::send_sms(&device, &number, &body)
                            .await
                            .err(),
                    )
                });
            }
            Message::SmsSent(error) => {
                if let Some(why) = error {
                    if let Some(Dialog::Sms { sending, .. }) = self.dialog.as_mut() {
                        *sending = false;
                    }
                    return self.toast(fl!("sms-failed", why = why));
                }
                self.dialog = None;
                return self.toast(fl!("sms-sent"));
            }
            Message::LogInteraction => {
                let Some(card) = self.head_card() else {
                    return Task::none();
                };
                if let Err(why) = self.crm.log_interaction(&card, "", chrono::Utc::now()) {
                    return self.toast(why);
                }
                // The nav's overdue count is now stale, and this person may
                // have just left the smart list.
                self.rebuild_nav();
                self.reload();
            }
            Message::SetCadence(index) => {
                let Some(card) = self.head_card() else {
                    return Task::none();
                };
                let days = crate::ui::crm::CADENCES
                    .get(index)
                    .copied()
                    .unwrap_or_default();
                if let Err(why) = self.crm.set_cadence(&card, Some(days)) {
                    return self.toast(why);
                }
                self.rebuild_nav();
                self.reload();
            }
            Message::AttachRequested => {
                if self.selected.is_none() {
                    return Task::none();
                }
                return cosmic::task::future(async {
                    use cosmic::dialog::file_chooser;

                    let dialog = file_chooser::open::Dialog::new().title(fl!("attach-file"));
                    match dialog.open_file().await {
                        Ok(response) => match response.url().to_file_path() {
                            Ok(path) => Message::AttachPath(path),
                            Err(()) => Message::DialogFailed(fl!("error-remote-file")),
                        },
                        Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                        Err(why) => Message::DialogFailed(why.to_string()),
                    }
                });
            }
            Message::AttachPath(path) => {
                let Some(card) = self.head_card() else {
                    return Task::none();
                };
                let attachment = match crate::attachments::store(&self.contacts_root, &path) {
                    Ok(attachment) => attachment,
                    Err(why) => return self.toast(why),
                };
                let name = attachment.name.clone();
                if let Err(why) = self.crm.attach(&card, attachment) {
                    return self.toast(why);
                }
                self.rebuild_nav();
                return self.toast(fl!("attachment-added", name = name));
            }
            Message::OpenAttachment(blob) => {
                let summary = crate::crm::summarise(&self.crm, &self.person_cards());
                let Some(attachment) = summary.attachments.iter().find(|a| a.blob == blob) else {
                    return Task::none();
                };
                let path = crate::attachments::path(&self.contacts_root, attachment);
                if !path.exists() {
                    // The blob directory is a plain directory; somebody may
                    // have cleaned it out from underneath us.
                    return self.toast(fl!("attachment-missing", name = attachment.name.clone()));
                }
                if let Err(why) = open::that_detached(&path) {
                    tracing::warn!(?path, %why, "could not open the attachment");
                    return self.toast(why.to_string());
                }
            }
            Message::DetachFile(blob) => {
                // The reference may sit on any of a linked person's cards.
                for card in self.person_cards() {
                    if let Err(why) = self.crm.detach(&card, &blob) {
                        return self.toast(why);
                    }
                }
                // Only now is the question answerable: the blob goes if no
                // record anywhere still names it.
                let referenced = self.crm.is_blob_referenced(&blob);
                if let Err(why) = crate::attachments::prune(&self.contacts_root, &blob, referenced)
                {
                    tracing::warn!(%why, "could not remove an unreferenced attachment");
                }
                self.rebuild_nav();
            }
            Message::NoteInput(text) => self.note_draft = text,
            Message::AddNote => {
                let Some(card) = self.head_card() else {
                    return Task::none();
                };
                let text = std::mem::take(&mut self.note_draft);
                if let Err(why) = self.crm.add_note(&card, &text, chrono::Utc::now()) {
                    // The note was not saved; losing what was typed as well
                    // would make the failure cost the user twice.
                    self.note_draft = text;
                    return self.toast(why);
                }
            }
            Message::RemoveNote(id) => {
                // The note may belong to any of a linked person's cards, so
                // ask each of them rather than only the head.
                for card in self.person_cards() {
                    if let Err(why) = self.crm.remove_note(&card, &id) {
                        return self.toast(why);
                    }
                }
            }
            Message::Unlink(key) => {
                if let Err(why) = self.links.unlink(&key.book, &key.uid) {
                    return self.toast(why);
                }
                // The card that just came out becomes its own row again, and
                // selecting it is what makes that visible.
                self.reload();
                self.selected = Some(key);
            }
            Message::LinkChecked => {
                if self.checked.len() < 2 {
                    return self.toast(fl!("link-needs-two"));
                }
                // In list order, so the topmost row becomes the precedence
                // head — the one the user sees first is the one that wins.
                let cards: Vec<crate::links::CardRef> = self
                    .contacts
                    .iter()
                    .map(ContactKey::of)
                    .filter(|key| self.checked.contains(key))
                    .map(|key| crate::links::CardRef {
                        book: key.book,
                        uid: key.uid,
                    })
                    .collect();
                let count = cards.len();
                let head = cards.first().cloned();
                if let Err(why) = self.links.link(cards) {
                    return self.toast(why);
                }
                self.selecting = false;
                self.checked.clear();
                self.reload();
                self.selected = head.map(|card| ContactKey {
                    book: card.book,
                    uid: card.uid,
                });
                return self.toast(fl!("linked-count", count = count));
            }
            Message::ReviewDuplicates => {
                if self.editor.is_some() || self.csv.is_some() || self.review.is_some() {
                    return Task::none();
                }
                let Some(store) = self.store.as_ref() else {
                    return Task::none();
                };
                // Over the whole address book, not the filtered list: a
                // duplicate the current filter hides is still a duplicate.
                let all: Vec<Contact> = store
                    .contacts()
                    .into_iter()
                    .filter(|c| !self.config.is_hidden(&c.addressbook_id))
                    .collect();
                let candidates = crate::dedupe::candidates(&all, &self.links);
                if candidates.is_empty() {
                    return self.toast(fl!("no-duplicates"));
                }
                self.review = Some(Review {
                    candidates,
                    cards: all,
                });
            }
            Message::ReviewLink(index) => {
                let Some(candidate) = self
                    .review
                    .as_ref()
                    .and_then(|review| review.candidates.get(index))
                    .cloned()
                else {
                    return Task::none();
                };
                if let Err(why) = self.links.link(vec![candidate.a, candidate.b]) {
                    return self.toast(why);
                }
                self.take_reviewed(index);
                self.reload();
            }
            Message::ReviewIgnore(index) => {
                let Some(candidate) = self
                    .review
                    .as_ref()
                    .and_then(|review| review.candidates.get(index))
                    .cloned()
                else {
                    return Task::none();
                };
                if let Err(why) = self.links.ignore(candidate.a, candidate.b) {
                    return self.toast(why);
                }
                self.take_reviewed(index);
            }
            Message::ReviewClose => self.review = None,

            Message::ExportSelectedRequested => {
                if self.checked.is_empty() {
                    return Task::none();
                }
                return cosmic::task::future(async move {
                    use cosmic::dialog::file_chooser::{self, FileFilter};

                    let dialog = file_chooser::save::Dialog::new()
                        .title(fl!("export"))
                        .file_name(format!("{}.vcf", fl!("app-title")))
                        .filter(FileFilter::new("vCard").glob("*.vcf"));

                    match dialog.save_file().await {
                        Ok(response) => match response.url().and_then(|u| u.to_file_path().ok()) {
                            Some(path) => Message::ExportSelectedTo(path),
                            None => Message::DialogFailed(fl!("error-remote-file")),
                        },
                        Err(file_chooser::Error::Cancelled) => Message::DialogCancelled,
                        Err(why) => Message::DialogFailed(why.to_string()),
                    }
                });
            }
            Message::ExportSelectedTo(path) => {
                // In the list's current order, not the set's arbitrary one, so
                // the file reads the way the window did.
                let mut text = String::new();
                for contact in self
                    .contacts
                    .iter()
                    .filter(|c| self.checked.contains(&ContactKey::of(c)))
                {
                    // This contact's own card, not the file it lives in — a
                    // `.vcf` may hold several people, and pushing `raw` would
                    // emit all of them once per person selected.
                    let card = if contact.raw.trim().is_empty() {
                        cosmic_pim_core::vcard::to_vcard_versioned(
                            contact,
                            cosmic_pim_core::vcard::WriteVersion::default(),
                        )
                    } else {
                        card_segment(&contact.raw, &contact.uid)
                    };
                    text.push_str(&card);
                    if !card.ends_with('\n') {
                        text.push_str("\r\n");
                    }
                }
                match std::fs::write(&path, text) {
                    Ok(()) => return self.toast(fl!("export-done", path = file_label(&path))),
                    Err(why) => return self.toast(format!("{}: {why}", file_label(&path))),
                }
            }

            Message::AddToGroupRequested => {
                if !self.checked.is_empty() {
                    self.dialog = Some(Dialog::AddToGroup {
                        name: String::new(),
                    });
                }
            }
            Message::AddToGroupName(name) => {
                if let Some(Dialog::AddToGroup { name: current }) = self.dialog.as_mut() {
                    *current = name;
                }
            }
            Message::AddToGroupConfirmed => return self.add_checked_to_group(),

            Message::DeleteRequested => {
                if self.editor.is_some() {
                    return Task::none();
                }
                // With a group active in the sidebar and nobody selected, the
                // delete targets the group card. A selected person always wins
                // — deleting a group because a row happened to be deselected
                // would be a nasty surprise the confirm dialog cannot fix.
                if self.selected.is_none()
                    && let Some(NavEntry::Group { book, uid }) =
                        self.nav.active_data::<NavEntry>().cloned()
                    && let Some(group) = self.store.as_ref().and_then(|s| s.contact(&book, &uid))
                {
                    self.dialog = Some(Dialog::ConfirmDeleteGroup {
                        key: ContactKey { book, uid },
                        name: group.label(),
                    });
                    return Task::none();
                }
                // Several rows ticked: confirm once for the lot. Eight rows
                // over three books is easy to misread, so this is the one
                // delete that still asks first.
                if self.selecting && !self.checked.is_empty() {
                    self.dialog = Some(Dialog::ConfirmDeleteMany {
                        keys: self.checked.iter().cloned().collect(),
                    });
                    return Task::none();
                }
                // A single contact deletes immediately, with an undo toast —
                // asking forgiveness beats asking permission when forgiveness
                // is one click and permission is a dialog every time.
                if let Some(contact) = self.selected_contact() {
                    let key = ContactKey::of(contact);
                    return self.delete_with_undo(vec![key]);
                }
            }
            Message::DeleteConfirmed => match self.dialog.take() {
                Some(Dialog::ConfirmDeleteMany { keys }) => return self.delete_with_undo(keys),
                Some(Dialog::ConfirmDeleteGroup { key, name }) => {
                    let Some(store) = self.store.as_mut() else {
                        return Task::none();
                    };
                    // The server-side coordinates live in the sidecar, which
                    // survives the local delete — but the file name has to be
                    // taken while the card still exists.
                    let root = store.root().to_path_buf();
                    let file_name = store
                        .contact(&key.book, &key.uid)
                        .map(|card| card.file_name);
                    let file_names: Vec<&str> = file_name.iter().map(String::as_str).collect();
                    let queued = match write_and_queue(&root, &key.book, &file_names, || {
                        store.delete(&key.book, &key.uid)
                    }) {
                        Ok(((), queued)) => queued,
                        Err(why) => {
                            return self.toast(fl!(
                                "error-delete",
                                name = name,
                                why = why.to_string()
                            ));
                        }
                    };
                    self.selected = None;
                    self.editor = None;
                    self.rebuild_nav();
                    self.reload();
                    if let Err(unqueued) = queued {
                        return self.toast_unqueued(unqueued);
                    }
                }
                _ => return Task::none(),
            },
            Message::DialogCancel => self.dialog = None,

            Message::NewGroupRequested => {
                self.dialog = Some(Dialog::NewGroup {
                    name: String::new(),
                });
            }
            Message::NewGroupName(name) => {
                if let Some(Dialog::NewGroup { name: current }) = self.dialog.as_mut() {
                    *current = name;
                }
            }
            Message::NewGroupConfirmed => {
                let Some(Dialog::NewGroup { name }) = self.dialog.take() else {
                    return Task::none();
                };
                if name.trim().is_empty() {
                    return Task::none();
                }
                let version = self.write_version();
                let Some(store) = self.store.as_mut() else {
                    return Task::none();
                };
                let Some(book) = self.config.new_card_book(store.books()) else {
                    return self.toast(fl!("error-no-writable-book"));
                };
                // The group card's file is named as it is written, and queued
                // with it under the book's lock.
                let root = store.root().to_path_buf();
                let queued = match write_and_queue_creating(&root, &book, &[], || {
                    store
                        .create_group(name.trim(), &book, version)
                        .map(|group| {
                            let file = group.file_name.clone();
                            (group, vec![file])
                        })
                }) {
                    Ok((_, queued)) => queued,
                    Err(why) => return self.toast(why.to_string()),
                };
                self.rebuild_nav();
                self.reload();
                if let Err(unqueued) = queued {
                    return self.toast_unqueued(unqueued);
                }
            }
        }
        Task::none()
    }

    /// Drops a reviewed pair, closing the screen when it was the last one —
    /// an empty review screen is a screen with nothing to say.
    fn take_reviewed(&mut self, index: usize) {
        if let Some(review) = self.review.as_mut() {
            if index < review.candidates.len() {
                review.candidates.remove(index);
            }
            if review.candidates.is_empty() {
                self.review = None;
            }
        }
    }

    /// The version new cards are written in, per settings.
    fn write_version(&self) -> cosmic_pim_core::vcard::WriteVersion {
        if self.config.prefer_vcard4 {
            cosmic_pim_core::vcard::WriteVersion::V4
        } else {
            cosmic_pim_core::vcard::WriteVersion::V3
        }
    }

    fn persist_config(&mut self) {
        let Some(handler) = self.config_handler.as_ref() else {
            tracing::warn!("no configuration handler; this setting will not persist");
            return;
        };
        if let Err(why) = self.config.write_entry(handler) {
            tracing::error!(%why, "could not save the configuration");
        }
    }

    fn toast(&mut self, message: String) -> Task<Message> {
        self.toasts
            .push(widget::Toast::new(message))
            .map(Into::into)
    }

    fn with_account_form(&mut self, f: impl FnOnce(&mut AccountForm)) {
        if let Some(form) = self.account_form.as_mut() {
            f(form);
        }
    }
}

/// Notices changes made by anything other than us — a sync run, `khard`, a text
/// editor — and reloads the list.
///
/// Without this the app is only correct until the moment something else touches
/// the vdir, and there is no indication anything is stale.
fn file_watch_subscription() -> Subscription<Message> {
    use cosmic::iced::futures::SinkExt;

    Subscription::run(|| {
        cosmic::iced::stream::channel(
            1,
            |mut output: cosmic::iced::futures::channel::mpsc::Sender<_>| async move {
                let root = cosmic_pim_core::store::contacts::default_root();

                match cosmic_pim_core::store::watcher::watch(&root) {
                    Ok((_watch, mut rx)) => {
                        while rx.recv().await.is_some() {
                            if output.send(Message::FilesChanged).await.is_err() {
                                break;
                            }
                        }
                    }
                    Err(why) => {
                        tracing::warn!(
                            %why,
                            "cannot watch the contacts directory; external changes will need a refresh"
                        );
                        // Park forever rather than returning: a finished stream
                        // would make iced restart the subscription in a tight
                        // loop.
                        std::future::pending::<()>().await;
                    }
                }
            },
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn contact(book: &str, uid: &str) -> Contact {
        let mut c = Contact::draft(book);
        c.uid = uid.to_owned();
        c
    }

    /// The same UID in two books is two people as far as the list is concerned;
    /// keying on the UID alone made them one row.
    #[test]
    fn a_key_distinguishes_the_same_uid_in_two_books() {
        let personal = contact("personal", "shared-uid");
        let work = contact("work", "shared-uid");

        let key = ContactKey::of(&personal);
        assert!(key.matches(&personal));
        assert!(!key.matches(&work));
    }

    #[test]
    fn a_key_round_trips_through_the_contact_it_came_from() {
        let c = contact("personal", "abc");
        let key = ContactKey::of(&c);
        assert_eq!(key.book, "personal");
        assert_eq!(key.uid, "abc");
    }

    /// The icon is Circle's own, and says so.
    ///
    /// This application has shipped another app's icon before — Slate's
    /// calendar, byte for byte the same file — so a contacts app showed a
    /// calendar in the menu, on the panel, in its own About page and in the
    /// software centre. Nothing caught it. It is the failure mode that passes
    /// every check worth having: the file was committed, correctly named,
    /// correctly sized, valid SVG, and installed exactly where it belonged.
    ///
    /// The realistic way it happens is a whole file copied from a sibling
    /// repository, and a copied file brings the comment of the app it came
    /// from. So each icon source names itself, and this asserts the bytes the
    /// About page embeds are the ones that carry Circle's name. It cannot
    /// prove the art is right — only a person looking at it can do that — but
    /// it does catch the copy.
    #[test]
    fn the_embedded_icon_identifies_itself_as_circles() {
        let svg = std::str::from_utf8(APP_ICON).expect("the icon is text");
        assert!(
            svg.contains(APP_ID),
            "the embedded icon does not name {APP_ID}; if it was copied from \
             another app it will name that one instead"
        );
    }
}
