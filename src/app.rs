// SPDX-License-Identifier: GPL-3.0-only

//! The Circle application shell.

use cosmic::app::{Core, Task, context_drawer};
use cosmic::cosmic_config::CosmicConfigEntry as _;
use cosmic::iced::keyboard::{Key, Modifiers};
use cosmic::iced::{Length, Subscription};
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

const APP_ID: &str = "io.github.entro314labs.Circle";
const REPOSITORY: &str = env!("CARGO_PKG_REPOSITORY");
const APP_ICON: &[u8] = include_bytes!("../resources/icons/hicolor/scalable/apps/icon.svg");

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
    /// Accounts and credentials, shared with Slate — one `accounts.toml` for
    /// the whole suite. `None` when the account store could not be opened; the
    /// address book still works, it just cannot sync.
    accounts: Option<cosmic_pim_accounts::AccountStore>,
    /// The in-progress "add an account" form on the Accounts page.
    account_form: Option<AccountForm>,
    /// A sync pass is in flight. One at a time: two passes racing on the same
    /// sidecar files is the bug this flag exists to prevent.
    syncing: bool,
    /// The last pass's per-account summaries, shown on the Accounts page.
    sync_status: Option<String>,
    nav: nav_bar::Model,
    /// Writable books, cached as parallel id/name vectors.
    ///
    /// `widget::dropdown` borrows its labels for the lifetime of the view, so
    /// they cannot be built inside `settings_view`; caching them here also
    /// keeps the list off the per-frame path. Rebuilt by `rebuild_nav`.
    writable_ids: Vec<String>,
    writable_names: Vec<String>,

    /// Everything matching the current query and book filter, sorted.
    contacts: Vec<Contact>,
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

    ToggleBook(String),
    SortByGivenName(bool),
    DefaultBook(usize),
    PreferVcard4(bool),
    SyncInterval(usize),

    AccountAddStart,
    AccountAddCancel,
    AccountAddConfirm,
    AccountNameChanged(String),
    AccountUrlChanged(String),
    AccountUsernameChanged(String),
    AccountPasswordChanged(String),
    AccountRemove(String),
    SyncNow,
    SyncFinished(Vec<String>, bool),

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

    LogInteraction,
    SetCadence(usize),
    NoteInput(String),
    AddNote,
    RemoveNote(String),
    PhonesFound(Vec<crate::kdeconnect::Device>),
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
            note_draft: String::new(),
            relations: Vec::new(),
            phones: Vec::new(),
            folded: HashMap::new(),
            review: None,
            accounts,
            account_form: None,
            syncing: false,
            sync_status: None,
            nav: nav_bar::Model::default(),
            writable_ids: Vec::new(),
            writable_names: Vec::new(),
            contacts: Vec::new(),
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
        model.rebuild_nav();
        model.reload();

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
                ),
                Message::ToggleContextPage(ContextPage::Accounts),
            )
            .title(fl!("accounts")),
        })
    }

    fn view(&self) -> Element<'_, Self::Message> {
        let spacing = cosmic::theme::spacing();

        if let Some(fatal) = &self.fatal {
            return widget::text::body(format!("{}\n\n{fatal}", fl!("error-load-contacts")))
                .apply(widget::container)
                .padding(spacing.space_m)
                .into();
        }

        let collapsed = self.is_collapsed();

        let select_toggle = widget::tooltip(
            widget::button::icon(crate::ui::icon("object-select-symbolic"))
                .class(if self.selecting {
                    cosmic::theme::Button::Suggested
                } else {
                    cosmic::theme::Button::Icon
                })
                .on_press(Message::ToggleSelecting),
            widget::text::body(fl!("select")),
            widget::tooltip::Position::Bottom,
        );

        let mut list_pane = widget::column::with_capacity(3)
            .spacing(spacing.space_xs)
            .padding(spacing.space_xs)
            .push(
                widget::row::with_capacity(2)
                    .align_y(cosmic::iced::Alignment::Center)
                    .spacing(spacing.space_xxs)
                    .push(
                        widget::search_input(fl!("search-contacts"), &self.query)
                            .id(search_id())
                            .on_input(Message::QueryChanged)
                            .on_clear(Message::QueryChanged(String::new()))
                            // Enter lands on the first match, so
                            // type-then-Enter reaches a person with no mouse.
                            .on_submit(|_| Message::SelectFirst),
                    )
                    .push(select_toggle),
            )
            .push(crate::ui::list::list(
                &self.contacts,
                self.selected.as_ref(),
                &self.query,
                &self.photos,
                self.selecting,
                &self.checked,
            ));
        if self.selecting {
            list_pane = list_pane.push(self.action_bar());
        }

        // The selected row's cards, composed into one person. Built here
        // rather than inside the closure because the renderer clones what it
        // draws — the element does not borrow this, so a local is enough.
        let cards = self.selected_cards();
        let person = crate::ui::person::compose(&cards);

        let detail_or_placeholder = |show_back: bool| -> Element<'_, Message> {
            match &person {
                Some(person) => {
                    let now = chrono::Utc::now();
                    let summary = crate::crm::summarise(&self.crm, &self.person_cards());
                    let mut stacked = widget::column::with_capacity(4)
                        .spacing(spacing.space_m)
                        .padding(spacing.space_s)
                        .push(crate::ui::list::detail(
                            person,
                            self.photos.get(&ContactKey::of(person.head)),
                            !self.phones.is_empty(),
                        ));
                    // Relationships sit with the card's own data, above the
                    // local-only sections — they come off the card, and the
                    // two kinds of fact should not look alike.
                    if let Some(section) = crate::ui::crm::relations(&self.relations) {
                        stacked = stacked.push(section);
                    }
                    let detail: Element<'_, Message> = widget::scrollable(
                        stacked
                            .push(crate::ui::crm::keep_in_touch(&summary, now))
                            .push(crate::ui::crm::notes(&summary, &self.note_draft, now)),
                    )
                    .height(Length::Fill)
                    .into();
                    if show_back {
                        widget::column::with_capacity(2)
                            .push(
                                widget::tooltip(
                                    widget::button::icon(crate::ui::icon("go-previous-symbolic"))
                                        .on_press(Message::BackToList),
                                    widget::text::body(fl!("back-to-list")),
                                    widget::tooltip::Position::Bottom,
                                )
                                .apply(widget::container)
                                .padding(spacing.space_xxs),
                            )
                            .push(detail)
                            .into()
                    } else {
                        detail
                    }
                }
                None => widget::container(
                    widget::text::body(fl!("no-selection"))
                        .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
                )
                .padding(spacing.space_m)
                .into(),
            }
        };

        // One pane or three. Collapsed, the editor and the CSV mapper win the
        // window outright (they carry their own cancel), then a selected
        // contact's detail with a back button, then the list.
        let content: Element<'_, Message> = if collapsed {
            match (&self.review, &self.csv, &self.editor) {
                (Some(review), _, _) => self.review_pane(review),
                (None, Some(state), _) => self.csv_pane(state),
                (None, None, Some(state)) => self.editor_pane(state),
                (None, None, None) if self.selected_contact().is_some() => {
                    detail_or_placeholder(true)
                }
                (None, None, None) => list_pane.width(Length::Fill).into(),
            }
        } else {
            let right: Element<'_, Message> = match (&self.review, &self.csv, &self.editor) {
                (Some(review), _, _) => self.review_pane(review),
                (None, Some(state), _) => self.csv_pane(state),
                (None, None, Some(state)) => self.editor_pane(state),
                (None, None, None) => detail_or_placeholder(false),
            };
            widget::row::with_capacity(3)
                .push(list_pane.width(Length::Fixed(320.0)))
                .push(widget::divider::vertical::default())
                .push(widget::container(right).width(Length::Fill))
                .into()
        };

        // The toaster overlays transient errors without stealing focus, which
        // is what a failed save needs: the editor is still open behind it and
        // the user's text is still there.
        widget::toaster(&self.toasts, content)
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
                    tracing::warn!(url, %why, "could not open the link");
                }
            }
            Message::ToggleContextPage(page) => {
                if self.context_page == page {
                    self.core.window.show_context = !self.core.window.show_context;
                } else {
                    self.context_page = page;
                    self.core.window.show_context = true;
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
            Message::SyncFinished(lines, changed) => {
                self.syncing = false;
                self.sync_status = Some(lines.join("\n"));
                if changed {
                    // Sync wrote `.vcf` files directly; everything read from
                    // them — the list, the nav, the photo cache — is stale.
                    if let Some(store) = self.store.as_mut() {
                        store.refresh();
                    }
                    self.photos.clear();
                    self.rebuild_nav();
                    self.reload();
                }
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
                let target = self
                    .config
                    .default_book
                    .as_deref()
                    .filter(|id| books.iter().any(|b| b.id == *id && !b.read_only))
                    .map(ToOwned::to_owned)
                    .or_else(|| store.default_book().map(|b| b.id.clone()));

                match target {
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
                if let Some(state) = self.editor.as_mut() {
                    state.update(message);
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
                    return self.toast(fl!("error-no-writable-book"));
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
            Message::PhonesFound(phones) => {
                if !phones.is_empty() {
                    tracing::info!(count = phones.len(), "KDE Connect devices in reach");
                }
                self.phones = phones;
            }
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
            Message::NoteInput(text) => self.note_draft = text,
            Message::AddNote => {
                let Some(card) = self.head_card() else {
                    return Task::none();
                };
                let text = std::mem::take(&mut self.note_draft);
                if let Err(why) = self.crm.add_note(&card, &text, chrono::Utc::now()) {
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
                    if contact.raw.trim().is_empty() {
                        text.push_str(&cosmic_pim_core::vcard::to_vcard_versioned(
                            contact,
                            cosmic_pim_core::vcard::WriteVersion::default(),
                        ));
                    } else {
                        text.push_str(&contact.raw);
                        if !contact.raw.ends_with('\n') {
                            text.push_str("\r\n");
                        }
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
                    let file_name = store.contact(&key.book, &key.uid).map(|c| c.file_name);
                    if let Err(why) = store.delete(&key.book, &key.uid) {
                        return self.toast(fl!("error-delete", name = name, why = why.to_string()));
                    }
                    if let Some(file_name) = file_name {
                        queue_removal(store, &key.book, &file_name);
                    }
                    self.selected = None;
                    self.editor = None;
                    self.rebuild_nav();
                    self.reload();
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
                let Some(book) = store.default_book().map(|b| b.id.clone()) else {
                    return self.toast(fl!("error-no-writable-book"));
                };
                match store.create_group(name.trim(), &book, version) {
                    Ok(group) => queue_push(store, &book, &group.file_name),
                    Err(why) => return self.toast(why.to_string()),
                }
                self.rebuild_nav();
                self.reload();
            }
        }
        Task::none()
    }

    /// Updates the header and window titles.
    fn update_title(&mut self) -> Task<Message> {
        let mut title = fl!("app-title");
        match self.nav.active_data::<NavEntry>() {
            Some(NavEntry::Book(id)) => {
                if let Some(book) = self.store.as_ref().and_then(|s| s.book(id)) {
                    title.push_str(" — ");
                    title.push_str(&book.name);
                }
            }
            Some(NavEntry::Category(category)) => {
                title.push_str(" — ");
                title.push_str(category);
            }
            Some(NavEntry::Group { book, uid }) => {
                if let Some(group) = self.store.as_ref().and_then(|s| s.contact(book, uid)) {
                    title.push_str(" — ");
                    title.push_str(&group.label());
                }
            }
            _ => {}
        }

        if let Some(id) = self.core.main_window_id() {
            self.set_window_title(title, id)
        } else {
            Task::none()
        }
    }

    /// Rebuilds the nav bar from the books on disk, keeping the active entry
    /// where it can be kept.
    ///
    /// Called after anything that can change the set of books — a sync creating
    /// a collection, a book being hidden in settings — because a nav bar built
    /// once at startup silently stops listing an address book that appears
    /// afterwards.
    fn rebuild_nav(&mut self) {
        self.rebuild_writable();
        let previous = self.nav.active_data::<NavEntry>().cloned();

        self.nav = nav_bar::Model::default();
        self.nav
            .insert()
            .text(fl!("all-contacts"))
            .data(NavEntry::All)
            .icon(crate::ui::icon("system-users-symbolic"));

        let books: Vec<CalendarMeta> = self
            .store
            .as_ref()
            .map(|s| s.books().to_vec())
            .unwrap_or_default();

        for book in books.iter().filter(|b| !self.config.is_hidden(&b.id)) {
            self.nav
                .insert()
                .text(book.name.clone())
                .data(NavEntry::Book(book.id.clone()))
                .icon(crate::ui::icon("avatar-default-symbolic"));
        }

        // The keep-in-touch smart list, above the groups. Hidden until
        // something has a cadence: an "Overdue" row that can only ever be
        // empty is a feature advertising itself at the user.
        if !self.crm.is_empty() {
            self.nav
                .insert()
                .text(fl!("keep-in-touch"))
                .data(NavEntry::Overdue)
                .icon(crate::ui::icon("alarm-symbolic"));
        }

        // Groups, read off the cards' CATEGORIES rather than kept anywhere:
        // the categories ARE the groups (tier 4's vCard-native half), so a
        // group with no members simply stops existing — nothing to garbage
        // collect, nothing to migrate.
        let mut categories: Vec<String> = self
            .store
            .as_ref()
            .map(|store| {
                store
                    .contacts()
                    .iter()
                    .filter(|c| !self.config.is_hidden(&c.addressbook_id))
                    .flat_map(|c| c.categories.iter().cloned())
                    .collect()
            })
            .unwrap_or_default();
        categories.sort();
        categories.dedup();
        for category in categories {
            self.nav
                .insert()
                .text(category.clone())
                .data(NavEntry::Category(category))
                .icon(crate::ui::icon("folder-symbolic"));
        }

        // Group cards, the other mechanism. Same section of the sidebar as
        // the CATEGORIES groups — a user thinks "my groups", not "my two
        // grouping mechanisms"; the distinction only matters to the code.
        let groups = self.store.as_ref().map(|s| s.groups()).unwrap_or_default();
        for group in groups
            .iter()
            .filter(|g| !self.config.is_hidden(&g.addressbook_id))
        {
            self.nav
                .insert()
                .text(group.label())
                .data(NavEntry::Group {
                    book: group.addressbook_id.clone(),
                    uid: group.uid.clone(),
                })
                .icon(crate::ui::icon("system-users-symbolic"));
        }

        // Restore the previous filter if that book still exists, else fall back
        // to "All" rather than leaving nothing active — `active_data` returning
        // `None` would filter the list down to nothing at all.
        let restored = previous.and_then(|previous| {
            self.nav
                .iter()
                .find(|id| self.nav.data::<NavEntry>(*id) == Some(&previous))
        });
        match restored {
            Some(id) => self.nav.activate(id),
            None => {
                self.nav.activate_position(0);
            }
        }
    }

    /// Re-reads the address book, applying the current book filter and query.
    ///
    /// Read straight from disk rather than cached: an address book is a few
    /// hundred kilobytes of text, and a cache that can go stale behind a
    /// vdirsyncer run is worse than the read it saves. See the note in
    /// `cosmic_pim_core::store::contacts`.
    fn reload(&mut self) {
        let Some(store) = self.store.as_ref() else {
            return;
        };

        let filter = self.nav.active_data::<NavEntry>().cloned();

        self.contacts = store
            .search(&self.query)
            .into_iter()
            .filter(|c| match &filter {
                Some(NavEntry::Book(id)) => c.addressbook_id == *id,
                Some(NavEntry::Category(category)) => {
                    c.categories.contains(category) && !self.config.is_hidden(&c.addressbook_id)
                }
                // "All contacts" still respects what the user hid: a book
                // unticked in settings should not come back through the front
                // door.
                _ => !self.config.is_hidden(&c.addressbook_id),
            })
            .collect();

        if let Some(NavEntry::Group { book, uid }) = &filter {
            // Member URIs resolve to UIDs where they can (`urn:uuid:…` and
            // bare values); a `mailto:` member names an address, not a card,
            // and cannot match a row.
            let member_uids: std::collections::HashSet<String> = self
                .store
                .as_ref()
                .and_then(|s| s.contact(book, uid))
                .map(|group| {
                    group
                        .members
                        .iter()
                        .filter_map(|uri| {
                            cosmic_pim_core::vcard::member_uid(uri).map(ToOwned::to_owned)
                        })
                        .collect()
                })
                .unwrap_or_default();
            self.contacts.retain(|c| member_uids.contains(&c.uid));
        }

        self.fold_links();

        if matches!(filter, Some(NavEntry::Overdue)) {
            let overdue = self.overdue_keys();
            self.contacts
                .retain(|contact| overdue.contains(&ContactKey::of(contact)));
        }

        if self.config.sort_by_given_name {
            self.contacts.sort_by_key(|c| {
                (
                    c.name.given.to_lowercase(),
                    c.name.family.to_lowercase(),
                    c.label().to_lowercase(),
                )
            });
        }

        // Drop a selection the query has filtered away, or the detail pane
        // keeps showing someone who is no longer in the list.
        if self
            .selected
            .as_ref()
            .is_some_and(|key| !self.contacts.iter().any(|c| key.matches(c)))
        {
            self.selected = None;
        }
        // Fill the photo cache for whatever the list now shows. Only entries
        // not already decoded cost anything, so a search keystroke that
        // narrows the list decodes nothing at all.
        for contact in &self.contacts {
            if !contact.has_photo {
                continue;
            }
            let key = ContactKey::of(contact);
            if self.photos.contains_key(&key) {
                continue;
            }
            match cosmic_pim_core::vcard::photo(&contact.raw) {
                // `is_renderable` is load-bearing, not defensive. Iced accepts
                // any bytes and only finds out it cannot decode them at draw
                // time, where it silently renders nothing — so a card with a
                // truncated or bogus PHOTO became an invisible row instead of
                // falling back to initials. Leaving the entry out of the cache
                // is what makes `avatar()` generate one.
                Some(photo @ cosmic_pim_core::vcard::Photo::Bytes { .. })
                    if photo.is_renderable() =>
                {
                    let cosmic_pim_core::vcard::Photo::Bytes { data, .. } = photo else {
                        unreachable!("matched the Bytes variant")
                    };
                    self.photos
                        .insert(key, widget::image::Handle::from_bytes(data));
                }
                Some(cosmic_pim_core::vcard::Photo::Bytes { .. }) => {
                    tracing::debug!(
                        contact = contact.label(),
                        "PHOTO is not a recognised image; showing initials"
                    );
                }
                // A remote avatar is never fetched — network access for a
                // contact photo is off by design (03: opt-in "if ever").
                Some(cosmic_pim_core::vcard::Photo::Uri(_)) | None => {}
            }
        }
    }

    /// Collapses every linked person in the list down to one row.
    ///
    /// The row that survives is the person's precedence head — the first card
    /// in the link record that the current filter left visible, so filtering
    /// to one book still shows that book's card rather than nothing. The
    /// others move to `folded`, where the detail pane composes them back in.
    fn fold_links(&mut self) {
        self.folded.clear();
        if self.links.persons().is_empty() {
            return;
        }

        // Head per person: first card in record order that is actually here.
        let mut heads: HashMap<String, ContactKey> = HashMap::new();
        for person in self.links.persons() {
            if let Some(card) = person.cards.iter().find(|card| {
                self.contacts
                    .iter()
                    .any(|c| c.addressbook_id == card.book && c.uid == card.uid)
            }) {
                heads.insert(
                    person.id.clone(),
                    ContactKey {
                        book: card.book.clone(),
                        uid: card.uid.clone(),
                    },
                );
            }
        }

        let mut kept = Vec::with_capacity(self.contacts.len());
        for contact in std::mem::take(&mut self.contacts) {
            let key = ContactKey::of(&contact);
            let head = self
                .links
                .person_of(&key.book, &key.uid)
                .and_then(|person| heads.get(&person.id));
            match head {
                Some(head) if *head != key => {
                    self.folded.entry(head.clone()).or_default().push(contact);
                }
                _ => kept.push(contact),
            }
        }
        self.contacts = kept;

        // A selection that just became a folded member follows its head,
        // rather than leaving the detail pane empty.
        if let Some(selected) = self.selected.clone()
            && !self.contacts.iter().any(|c| selected.matches(c))
            && let Some(person) = self.links.person_of(&selected.book, &selected.uid)
            && let Some(head) = heads.get(&person.id)
        {
            self.selected = Some(head.clone());
        }
    }

    /// The cards behind the selected row, head first, each with its book's
    /// display name — what [`crate::ui::person::compose`] reads.
    fn selected_cards(&self) -> Vec<(&Contact, &str)> {
        match self.selected.as_ref() {
            Some(key) => self.cards_for(key),
            None => Vec::new(),
        }
    }

    /// [`Self::selected_cards`] for any row, not just the selected one.
    fn cards_for(&self, key: &ContactKey) -> Vec<(&Contact, &str)> {
        let Some(head) = self.contacts.iter().find(|c| key.matches(c)) else {
            return Vec::new();
        };
        let book_name = |contact: &Contact| {
            self.store
                .as_ref()
                .and_then(|store| store.book(&contact.addressbook_id))
                .map_or("", |book| book.name.as_str())
        };

        let mut cards = vec![(head, book_name(head))];
        if let Some(others) = self.folded.get(&ContactKey::of(head)) {
            // Record order, not list order: precedence is what the user chose
            // when linking, and the fold preserved it.
            let person = self.links.person_of(&head.addressbook_id, &head.uid);
            let position = |contact: &Contact| {
                person.and_then(|person| {
                    person.cards.iter().position(|card| {
                        card.book == contact.addressbook_id && card.uid == contact.uid
                    })
                })
            };
            let mut others: Vec<&Contact> = others.iter().collect();
            others.sort_by_key(|c| position(c).unwrap_or(usize::MAX));
            cards.extend(others.into_iter().map(|c| (c, book_name(c))));
        }
        cards
    }

    fn selected_contact(&self) -> Option<&Contact> {
        let key = self.selected.as_ref()?;
        self.contacts.iter().find(|c| key.matches(c))
    }

    /// Re-resolves the selected person's relationships.
    ///
    /// Against every contact in a visible book, not the filtered list: a
    /// relationship to somebody the current search hides is still a
    /// relationship, and one that silently stopped resolving when you typed
    /// would look like the card had changed.
    fn refresh_relations(&mut self) {
        self.relations.clear();
        let Some(raw) = self.selected_contact().map(|c| c.raw.clone()) else {
            return;
        };
        if !raw.contains("RELATED") && !raw.contains("X-ABRELATEDNAMES") {
            return; // The overwhelmingly common case; skip the whole-book read.
        }
        let Some(store) = self.store.as_ref() else {
            return;
        };
        let others: Vec<Contact> = store
            .contacts()
            .into_iter()
            .filter(|c| !self.config.is_hidden(&c.addressbook_id))
            .collect();
        self.relations = crate::relations::relations(&raw, &others);
    }

    /// The selected person's cards as link-store coordinates, head first.
    ///
    /// The head is what a new note, an interaction, or a cadence is recorded
    /// against — every CRM write lands on exactly one card, the same rule
    /// editing follows.
    fn person_cards(&self) -> Vec<crate::links::CardRef> {
        self.selected_cards()
            .into_iter()
            .map(|(contact, _)| crate::links::CardRef {
                book: contact.addressbook_id.clone(),
                uid: contact.uid.clone(),
            })
            .collect()
    }

    fn head_card(&self) -> Option<crate::links::CardRef> {
        self.person_cards().into_iter().next()
    }

    /// Everybody past their cadence, as of now.
    ///
    /// Recomputed rather than cached: it depends on the clock, so a cached
    /// answer is wrong by definition the moment it is stored.
    fn overdue_keys(&self) -> Vec<ContactKey> {
        if self.crm.is_empty() {
            return Vec::new();
        }
        let now = chrono::Utc::now();
        self.contacts
            .iter()
            .filter(|contact| {
                let key = ContactKey::of(contact);
                let cards = self.cards_of(&key);
                crate::crm::summarise(&self.crm, &cards).is_overdue(now)
            })
            .map(ContactKey::of)
            .collect()
    }

    /// [`Self::person_cards`] for any row, not just the selected one.
    fn cards_of(&self, key: &ContactKey) -> Vec<crate::links::CardRef> {
        self.cards_for(key)
            .into_iter()
            .map(|(contact, _)| crate::links::CardRef {
                book: contact.addressbook_id.clone(),
                uid: contact.uid.clone(),
            })
            .collect()
    }

    /// Whether the window is too narrow for panes side by side.
    fn is_collapsed(&self) -> bool {
        self.width < COLLAPSE_WIDTH
    }

    /// Ticks every row between the selection anchor and `key`, inclusive, in
    /// the list's current order — what Shift+click means everywhere else.
    fn check_range_to(&mut self, key: &ContactKey) {
        let position = |k: &ContactKey| self.contacts.iter().position(|c| k.matches(c));
        let (Some(anchor), Some(target)) =
            (self.selected.as_ref().and_then(&position), position(key))
        else {
            self.checked.insert(key.clone());
            return;
        };
        let (from, to) = (anchor.min(target), anchor.max(target));
        for contact in &self.contacts[from..=to] {
            self.checked.insert(ContactKey::of(contact));
        }
    }

    /// Deletes the given contacts immediately and offers one undo toast for
    /// the lot. The cards' bytes are kept until the toast dies, so undo is a
    /// byte-for-byte restore, not a reconstruction.
    fn delete_with_undo(&mut self, keys: Vec<ContactKey>) -> Task<Message> {
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        let mut removed = Vec::new();
        let mut label = String::new();
        let mut first_error: Option<String> = None;
        for key in &keys {
            let Some(contact) = store.contact(&key.book, &key.uid) else {
                continue;
            };
            if let Err(why) = store.delete(&key.book, &key.uid) {
                first_error.get_or_insert_with(|| {
                    fl!(
                        "error-delete",
                        name = contact.label(),
                        why = why.to_string()
                    )
                });
                continue;
            }
            queue_removal(store, &key.book, &contact.file_name);
            self.photos.remove(key);
            label = contact.label();
            let card = crate::links::CardRef {
                book: key.book.clone(),
                uid: key.uid.clone(),
            };
            let crm = self.crm.record(&card).cloned();
            if let Err(why) = self.crm.forget(&card) {
                tracing::warn!(%why, "could not remove the notes for a deleted contact");
            }
            removed.push(DeletedCard {
                book: key.book.clone(),
                uid: key.uid.clone(),
                file_name: contact.file_name,
                raw: contact.raw,
                crm,
            });
        }

        self.selected = None;
        self.selecting = false;
        self.checked.clear();
        self.rebuild_nav();
        self.reload();

        if let Some(why) = first_error {
            return self.toast(why);
        }
        if removed.is_empty() {
            return Task::none();
        }

        let message = if removed.len() == 1 {
            fl!("deleted-one", name = label)
        } else {
            fl!("deleted-many", count = removed.len())
        };
        self.undo_seq += 1;
        let token = self.undo_seq;
        self.undo.insert(token, removed);
        while self.undo.len() > UNDO_DEPTH {
            let oldest = *self.undo.keys().next().unwrap_or(&token);
            self.undo.remove(&oldest);
        }
        self.toasts
            .push(
                widget::Toast::new(message)
                    .action(fl!("undo"), move |_| Message::UndoDelete(token)),
            )
            .map(Into::into)
    }

    /// Puts a deletion's cards back, byte for byte, and re-queues them for
    /// upload — the mirror image of [`Self::delete_with_undo`].
    fn undo_delete(&mut self, token: u64) -> Task<Message> {
        let Some(cards) = self.undo.remove(&token) else {
            return Task::none();
        };
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        let mut first_error = None;
        for card in cards {
            let Some(meta) = store.book(&card.book).cloned() else {
                first_error.get_or_insert_with(|| fl!("error-load-contacts"));
                continue;
            };
            match cosmic_pim_core::store::contacts::write_contact_raw(
                &meta,
                &card.file_name,
                &card.raw,
            ) {
                Ok(()) => {
                    queue_push(store, &card.book, &card.file_name);
                    if let Some(record) = card.crm {
                        let card = crate::links::CardRef {
                            book: card.book.clone(),
                            uid: card.uid.clone(),
                        };
                        if let Err(why) = self.crm.restore(&card, record) {
                            tracing::warn!(%why, "could not put back the notes for an undone delete");
                        }
                    }
                }
                Err(why) => {
                    first_error.get_or_insert_with(|| why.to_string());
                }
            }
        }

        self.rebuild_nav();
        self.reload();
        if let Some(why) = first_error {
            return self.toast(why);
        }
        Task::none()
    }

    /// Adds every checked contact to the named `CATEGORIES` group — the
    /// bulk twin of the editor's categories field.
    fn add_checked_to_group(&mut self) -> Task<Message> {
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
                    queue_push_with_base(
                        store,
                        &key.book,
                        &contact.file_name,
                        (!base.trim().is_empty()).then_some(base.as_str()),
                    );
                    joined += 1;
                }
                Err(why) => {
                    first_error.get_or_insert_with(|| {
                        fl!("error-save", name = contact.label(), why = why.to_string())
                    });
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

    /// The bar under the list while selecting: the count and what can be done
    /// with the set.
    fn action_bar(&self) -> Element<'_, Message> {
        let spacing = cosmic::theme::spacing();
        let count = self.checked.len();

        let mut group = widget::button::standard(fl!("add-to-group"));
        let mut export = widget::button::standard(fl!("export"));
        let mut delete = widget::button::destructive(fl!("delete"));
        // Linking needs two rows to be a question at all.
        let mut link = widget::button::standard(fl!("link"));
        if count > 0 {
            group = group.on_press(Message::AddToGroupRequested);
            export = export.on_press(Message::ExportSelectedRequested);
            delete = delete.on_press(Message::DeleteRequested);
        }
        if count > 1 {
            link = link.on_press(Message::LinkChecked);
        }

        widget::column::with_capacity(2)
            .spacing(spacing.space_xxs)
            .push(widget::text::caption(fl!("selected-count", count = count)))
            .push(
                widget::flex_row(vec![
                    link.into(),
                    group.into(),
                    export.into(),
                    delete.into(),
                ])
                .spacing(spacing.space_xxs)
                .row_spacing(spacing.space_xxs),
            )
            .into()
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

    fn writable_books(&self) -> Vec<CalendarMeta> {
        self.store
            .as_ref()
            .map(|s| s.books().iter().filter(|b| !b.read_only).cloned().collect())
            .unwrap_or_default()
    }

    fn rebuild_writable(&mut self) {
        let books = self.writable_books();
        self.writable_ids = books.iter().map(|b| b.id.clone()).collect();
        self.writable_names = books.iter().map(|b| b.name.clone()).collect();
    }

    /// The membership rows for the editor: every group card in `book`, marked
    /// with whether `uid` is currently a member.
    fn group_rows(&self, book: &str, uid: Option<&str>) -> Vec<editor::GroupRow> {
        use cosmic_pim_core::vcard::member_uid;

        self.store
            .as_ref()
            .map(|store| {
                store
                    .groups()
                    .into_iter()
                    .filter(|g| g.addressbook_id == book)
                    .map(|g| {
                        let member = uid.is_some_and(|uid| {
                            g.members.iter().any(|uri| member_uid(uri) == Some(uid))
                        });
                        editor::GroupRow {
                            uid: g.uid,
                            name: g.display_name,
                            member,
                            was_member: member,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default()
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

    /// Imports the cards in a `.vcf` file into the default book, UID-keyed so
    /// re-importing updates rather than duplicates.
    fn import(&mut self, path: &std::path::Path) -> Task<Message> {
        let Some(book_id) = self
            .config
            .default_book
            .clone()
            .filter(|id| {
                self.store
                    .as_ref()
                    .and_then(|s| s.book(id))
                    .is_some_and(|b| !b.read_only)
            })
            .or_else(|| {
                self.store
                    .as_ref()
                    .and_then(|s| s.default_book())
                    .map(|b| b.id.clone())
            })
        else {
            return self.toast(fl!("error-no-writable-book"));
        };

        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(why) => return self.toast(format!("{}: {why}", file_label(path))),
        };
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        match store.import_vcf(&text, &book_id) {
            Ok(summary) if summary.total() == 0 => {
                self.toast(fl!("import-empty", path = file_label(path)))
            }
            Ok(summary) => {
                for file in &summary.files {
                    queue_push(store, &book_id, file);
                }
                // An updated card may carry a new photo under an old key.
                self.photos.clear();
                self.reload();
                self.toast(fl!(
                    "import-done",
                    added = summary.added.to_string(),
                    updated = summary.updated.to_string()
                ))
            }
            Err(why) => self.toast(why.to_string()),
        }
    }

    /// Commits the editor to the store.
    fn save_editor(&mut self) -> Task<Message> {
        let Some(state) = self.editor.as_ref() else {
            return Task::none();
        };
        if !state.is_saveable() {
            return Task::none();
        }

        let contact = state.finish();
        let photo_edit = state.photo.clone();
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
            // user's text is the only copy that exists.
            return self.toast(fl!(
                "error-save",
                name = contact.label(),
                why = why.to_string()
            ));
        }

        // The save landed — queue it for upload before anything later in this
        // function can fail. The photo patch below rewrites the same file, so
        // one queue entry covers both. The base is the text the editor was
        // opened on — exactly what this edit was made against — which is what
        // lets sync auto-merge if the server changed the card meanwhile; a
        // brand-new contact has no before and queues without one.
        if let Some(saved) = store.contact(&contact.addressbook_id, &contact.uid) {
            let base = (!contact.raw.trim().is_empty()).then_some(contact.raw.as_str());
            queue_push_with_base(store, &contact.addressbook_id, &saved.file_name, base);
        }

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
            return self.toast(fl!("error-photo", why = why));
        }

        // Membership lives on the GROUP cards, so the changed rows patch those
        // — only the changed ones, or every contact save would churn every
        // group file and push them all to the server unchanged.
        let membership_error = apply_group_changes(store, &contact, &changed_groups);

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

    /// Validates the add-account form and stores the account.
    fn confirm_account(&mut self) -> Task<Message> {
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

    /// Runs a sync pass off the UI thread.
    fn sync_now(&mut self) -> Task<Message> {
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
                    Err(why) => return (vec![why.to_string()], false),
                };
                let reports = cosmic_pim_sync::sync_all(
                    &mut accounts,
                    &registry,
                    &calendar_root,
                    &contacts_root,
                );
                let changed = reports.iter().any(cosmic_pim_sync::AccountReport::changed);
                (
                    reports
                        .iter()
                        .map(cosmic_pim_sync::AccountReport::summary)
                        .collect(),
                    changed,
                )
            })
            .await
            .unwrap_or_else(|why| (vec![why.to_string()], false));

            Message::SyncFinished(outcome.0, outcome.1)
        })
    }

    /// Commits the CSV mapping: every row becomes a contact in the default
    /// book; a row whose mapped UID already exists updates that contact
    /// through the patcher instead of duplicating it.
    fn import_csv(&mut self) -> Task<Message> {
        let Some(state) = self.csv.as_ref() else {
            return Task::none();
        };
        if !state.is_importable() {
            return Task::none();
        }
        let version = self.write_version();
        let Some(book_id) = self
            .store
            .as_ref()
            .and_then(|s| s.default_book())
            .map(|b| b.id.clone())
        else {
            return self.toast(fl!("error-no-writable-book"));
        };

        let (contacts, skipped) = state.contacts(&book_id);
        let Some(store) = self.store.as_mut() else {
            return Task::none();
        };

        let mut added = 0usize;
        let mut updated = 0usize;
        for mut contact in contacts {
            // A mapped UID that already exists means "update that contact":
            // adopt its file and raw bytes so the save patches losslessly.
            if let Some(existing) = store.contact(&book_id, &contact.uid) {
                contact.file_name = existing.file_name;
                contact.raw = existing.raw;
                updated += 1;
            } else {
                added += 1;
            }
            if let Err(why) = store.save_as(&contact, version) {
                self.csv = None;
                self.reload();
                return self.toast(fl!(
                    "error-save",
                    name = contact.label(),
                    why = why.to_string()
                ));
            }
            if let Some(saved) = store.contact(&book_id, &contact.uid) {
                // An updated row adopted the existing card's bytes above;
                // those are its base. An added row has no before.
                let base = (!contact.raw.trim().is_empty()).then_some(contact.raw.as_str());
                queue_push_with_base(store, &book_id, &saved.file_name, base);
            }
        }

        self.csv = None;
        self.rebuild_nav();
        self.reload();
        self.toast(fl!(
            "csv-import-done",
            added = added.to_string(),
            updated = updated.to_string(),
            skipped = skipped.to_string()
        ))
    }

    /// The duplicate review pane, with its own header and close button.
    fn review_pane<'a>(&'a self, review: &'a Review) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let bar = widget::row::with_capacity(3)
            .align_y(cosmic::iced::Alignment::Center)
            .spacing(spacing.space_xs)
            .push(widget::text::title4(fl!("review-duplicates")))
            .push(widget::text::caption(fl!(
                "review-remaining",
                count = review.candidates.len()
            )))
            .push(widget::Space::new().width(Length::Fill))
            .push(widget::button::standard(fl!("close")).on_press(Message::ReviewClose));

        // Resolve each pair's cards here: the screen has no store, and a card
        // deleted underneath it simply stops being drawn.
        let resolved: Vec<Option<(crate::ui::review::Side<'_>, crate::ui::review::Side<'_>)>> =
            review
                .candidates
                .iter()
                .map(|candidate| {
                    Some((
                        self.side(review, &candidate.a)?,
                        self.side(review, &candidate.b)?,
                    ))
                })
                .collect();

        widget::column::with_capacity(2)
            .spacing(spacing.space_s)
            .padding(spacing.space_s)
            .push(bar)
            .push(crate::ui::review::view(
                &review.candidates,
                &resolved,
                Message::ReviewLink,
                Message::ReviewIgnore,
            ))
            .into()
    }

    /// One side of a review pair, resolved against the loaded books.
    fn side<'a>(
        &'a self,
        review: &'a Review,
        card: &crate::links::CardRef,
    ) -> Option<crate::ui::review::Side<'a>> {
        // From the screen's own snapshot, not the filtered list: review works
        // over the whole address book, and the list may be showing one book.
        let contact = review
            .cards
            .iter()
            .find(|c| c.addressbook_id == card.book && c.uid == card.uid)?;
        Some(crate::ui::review::Side {
            contact,
            book: self
                .store
                .as_ref()
                .and_then(|store| store.book(&card.book))
                .map_or("", |book| book.name.as_str()),
        })
    }

    /// The CSV mapping pane, with its own import/cancel bar.
    fn csv_pane<'a>(&'a self, state: &'a csv::State) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let mut import = widget::button::suggested(fl!("import"));
        if state.is_importable() {
            import = import.on_press(Message::CsvConfirm);
        }

        let bar = widget::row::with_capacity(4)
            .align_y(cosmic::iced::Alignment::Center)
            .spacing(spacing.space_xs)
            .push(widget::text::title4(fl!("import-csv")))
            .push(widget::Space::new().width(Length::Fill))
            .push(widget::button::standard(fl!("cancel")).on_press(Message::CsvCancel))
            .push(import);

        widget::column::with_capacity(2)
            .spacing(spacing.space_s)
            .padding(spacing.space_s)
            .push(bar)
            .push(csv::view(state).map(Message::Csv))
            .into()
    }

    /// The editor pane, with its own save/cancel bar.
    fn editor_pane<'a>(&'a self, state: &'a editor::State) -> Element<'a, Message> {
        let spacing = cosmic::theme::spacing();

        let title = if state.is_new {
            fl!("new-contact")
        } else {
            fl!("edit-contact")
        };

        let mut save = widget::button::suggested(fl!("save"));
        // Disabled rather than hidden, and disabled only for the one reason a
        // save can be refused: a card with no name at all would be a row nobody
        // could find again.
        if state.is_saveable() {
            save = save.on_press(Message::EditorSave);
        }

        let mut bar = widget::row::with_capacity(5)
            .align_y(cosmic::iced::Alignment::Center)
            .spacing(spacing.space_xs)
            .push(widget::text::title4(title));

        // Editing a linked person edits exactly one of its cards — the head.
        // Saying which one is not decoration: the detail pane showed values
        // from several books, and only this card's are in the fields below.
        if !state.is_new
            && self
                .links
                .person_of(&state.contact.addressbook_id, &state.contact.uid)
                .is_some()
            && let Some(book) = self
                .store
                .as_ref()
                .and_then(|store| store.book(&state.contact.addressbook_id))
        {
            bar = bar.push(
                widget::text::caption(fl!("editing-card", book = book.name.clone()))
                    .class(cosmic::theme::Text::Custom(crate::ui::dim_text)),
            );
        }

        let bar = bar
            .push(widget::Space::new().width(Length::Fill))
            .push(widget::button::standard(fl!("cancel")).on_press(Message::EditorCancel))
            .push(save);

        widget::column::with_capacity(2)
            .spacing(spacing.space_s)
            .padding(spacing.space_s)
            .push(bar)
            .push(editor::view(state).map(Message::Editor))
            .into()
    }
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
            set_photo(&saved.raw, &data, mime).ok_or_else(|| fl!("error-load-contacts"))?
        }
        editor::PhotoEdit::Remove => {
            let saved = store
                .contact(&contact.addressbook_id, &contact.uid)
                .ok_or_else(|| fl!("error-load-contacts"))?;
            match remove_photo(&saved.raw) {
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
            Ok(()) => queue_push_with_base(
                store,
                &contact.addressbook_id,
                &group.file_name,
                Some(&group.raw),
            ),
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

/// Queues a written card for upload to whatever server its book is bound to.
///
/// Storage deliberately knows nothing about CardDAV (see
/// `cosmic_pim_sync::writeback`), so every write site in this file pairs its
/// save with this call. A local-only book queues nothing, and a failure to
/// queue is a warning rather than an error: the local save already succeeded,
/// and the card's text is not at risk.
///
/// Sites that read the card before overwriting it call
/// [`queue_push_with_base`] instead: the pre-edit bytes are what let the sync
/// engine three-way-merge automatically when the server turns out to have
/// changed the same card. This form is for writes with no meaningful "before"
/// — a brand-new file, an undo restoring a deleted one, a bulk import.
fn queue_push(store: &ContactStore, book_id: &str, file_name: &str) {
    queue_push_with_base(store, book_id, file_name, None);
}

/// [`queue_push`], carrying the card's pre-edit bytes.
///
/// `base` is what the file held when the caller read it — the text the edit
/// was made against. The queue keeps the base from the first enqueue only, so
/// stacked unsent edits keep the oldest base (the last text the server
/// acknowledged) without any bookkeeping here.
fn queue_push_with_base(store: &ContactStore, book_id: &str, file_name: &str, base: Option<&str>) {
    if let Err(why) = cosmic_pim_sync::queue_save_with_base(store.root(), book_id, file_name, base)
    {
        tracing::warn!(book_id, file_name, %why, "could not queue the save for upload");
    }
}

/// The delete-side twin of [`queue_push`].
fn queue_removal(store: &ContactStore, book_id: &str, file_name: &str) {
    if let Err(why) = cosmic_pim_sync::queue_delete(store.root(), book_id, file_name) {
        tracing::warn!(book_id, file_name, %why, "could not queue the deletion for upload");
    }
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

/// A path's file name, for messages — the full path is noise in a toast.
fn file_label(path: &std::path::Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
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
