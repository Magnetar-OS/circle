// SPDX-License-Identifier: GPL-3.0-only

//! Editing a contact.
//!
//! # What this is allowed to touch
//!
//! The editor works on a clone of the [`Contact`] and hands it back on save;
//! [`cosmic_pim_core::store::contacts::write_contact`] then *patches* the
//! stored vCard rather than re-serialising it, so PHOTO, GEO, IMPP, `X-`
//! properties and everything else Circle does not model survive the edit.
//!
//! That patching has one boundary worth understanding, because it shows up in
//! this file as a deliberately reduced set of controls.
//!
//! # Grouped entries
//!
//! Apple-style cards attach a custom label to a value by grouping two lines:
//!
//! ```text
//! item1.EMAIL;type=INTERNET:ada@home.example
//! item1.X-ABLabel:Summer house
//! ```
//!
//! The substrate's patcher edits a grouped line's **value in place** and leaves
//! the group alone, because rewriting it as an ungrouped `EMAIL` line would
//! orphan the label — and dropping it from the list does not delete it, since
//! the patcher only replaces ungrouped lines.
//!
//! So a grouped entry here gets an editable value, its custom label shown as
//! text, and no remove button. Offering a remove that silently did nothing
//! would be worse than not offering one: the user would believe the address was
//! gone and it would reappear on the next read.

use cosmic::Element;
use cosmic::iced::Length;
use cosmic::widget;
use cosmic_pim_core::model::{Address, CalendarMeta, Contact, Typed};

use crate::fl;

/// Labels offered in the type dropdown.
///
/// vCard `TYPE` is free text and servers ship their own, so a value already on
/// the card that is not in this list is appended rather than replaced — picking
/// the nearest standard label would quietly discard what the user chose
/// somewhere else.
const LABELS: &[&str] = &["home", "work", "mobile", "other"];

/// Which single-value field a text message targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Field {
    DisplayName,
    Given,
    Family,
    Additional,
    Prefix,
    Suffix,
    Organisation,
    JobTitle,
    Note,
    Birthday,
    Categories,
}

/// Which repeating list a message targets.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ListKind {
    Email,
    Phone,
    Url,
    Nickname,
}

/// A component of a structured postal address.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum AddressPart {
    Street,
    Extended,
    Locality,
    Region,
    PostalCode,
    Country,
}

/// What should happen to the card's photo on save.
///
/// Held as an intent rather than applied live because the photo lives in the
/// card's raw bytes, which a *new* contact does not have until its first save
/// — the shell applies this after the card exists. See `AppModel::save_editor`.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub enum PhotoEdit {
    /// Leave whatever the card carries.
    #[default]
    Keep,
    /// Replace with the image file at this path.
    Set(std::path::PathBuf),
    /// Strip the PHOTO property.
    Remove,
}

#[derive(Clone, Debug)]
pub enum Message {
    Text(Field, String),
    ListValue(ListKind, usize, String),
    ListLabel(ListKind, usize, usize),
    ListPreferred(ListKind, usize),
    ListRemove(ListKind, usize),
    ListAdd(ListKind),
    AddressPart(usize, AddressPart, String),
    AddressRemove(usize),
    AddressAdd,
    Book(usize),
    /// Membership toggled for the group row at this index.
    GroupToggled(usize, bool),
    /// Asks the shell to open its file dialog — the dialog is async and the
    /// shell owns the async runtime, so the editor only raises its hand.
    PhotoPickRequested,
    /// The shell's answer.
    PhotoChosen(std::path::PathBuf),
    PhotoRemove,
    PhotoKeep,
}

/// One group the contact could belong to, and whether it does.
///
/// `was_member` is kept beside `member` so the shell can apply only the
/// *changes* on save — rewriting every group's member list on every contact
/// save would churn files (and sync pushes) that did not change.
#[derive(Clone, Debug)]
pub struct GroupRow {
    pub uid: String,
    pub name: String,
    pub member: bool,
    pub was_member: bool,
}

/// The editor's working state: a contact being edited or created.
pub struct State {
    /// The working copy. Committed to the store only on save.
    pub contact: Contact,
    /// Whether this contact does not exist on disk yet.
    pub is_new: bool,
    /// The birthday as typed.
    ///
    /// Held as text rather than parsed on every keystroke so that "1970-1" —
    /// a date halfway through being typed — is not thrown away as invalid.
    /// Parsed once, on save.
    pub birthday_text: String,
    /// Categories as typed, comma-separated. Same reason as the birthday: split
    /// on save, not while the user is still between two commas.
    pub categories_text: String,
    /// Ids of the books a new contact may be filed in, in dropdown order.
    pub books: Vec<String>,
    /// Human names for `books`, which `widget::dropdown` needs as a slice.
    pub book_names: Vec<String>,
    /// The pending photo change, applied by the shell on save.
    pub photo: PhotoEdit,
    /// Membership in the book's `KIND:group` cards, applied by the shell on
    /// save — the membership lives on the group cards, not on this contact.
    pub groups: Vec<GroupRow>,
}

impl State {
    /// Opens the editor on an existing contact.
    #[must_use]
    pub fn edit(contact: Contact, books: &[CalendarMeta]) -> Self {
        let birthday_text = contact
            .birthday
            .map(|b| b.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let categories_text = contact.categories.join(", ");
        let (ids, names) = writable(books);
        Self {
            contact,
            is_new: false,
            birthday_text,
            categories_text,
            books: ids,
            book_names: names,
            photo: PhotoEdit::Keep,
            groups: Vec::new(),
        }
    }

    /// Opens the editor on a blank contact filed in `book_id`.
    #[must_use]
    pub fn create(book_id: &str, books: &[CalendarMeta]) -> Self {
        let (ids, names) = writable(books);
        Self {
            contact: Contact::draft(book_id),
            is_new: true,
            birthday_text: String::new(),
            categories_text: String::new(),
            books: ids,
            book_names: names,
            photo: PhotoEdit::Keep,
            groups: Vec::new(),
        }
    }

    /// Fills the group rows — called by the shell, which owns the store.
    pub fn with_groups(mut self, groups: Vec<GroupRow>) -> Self {
        self.groups = groups;
        self
    }

    /// The membership rows whose state the user changed.
    #[must_use]
    pub fn changed_groups(&self) -> Vec<&GroupRow> {
        self.groups
            .iter()
            .filter(|g| g.member != g.was_member)
            .collect()
    }

    /// Whether the contact carries enough to be worth saving.
    ///
    /// `FN` is required by RFC 6350 and [`Contact::label`] derives one from the
    /// structured name, the organisation, or the first email — so anything that
    /// produces a non-empty label is enough. A card with no label at all would
    /// be an unnamed row nobody could ever find again.
    #[must_use]
    pub fn is_saveable(&self) -> bool {
        !self.contact.label().trim().is_empty()
    }

    /// Folds the free-text fields back into the model and returns it.
    ///
    /// An unparseable birthday clears the field rather than blocking the save:
    /// the text is visible in the editor, and refusing to save a contact
    /// because of a mistyped date would strand every other edit on the card.
    #[must_use]
    pub fn finish(&self) -> Contact {
        let mut contact = self.contact.clone();

        contact.birthday = if self.birthday_text.trim().is_empty() {
            None
        } else {
            chrono::NaiveDate::parse_from_str(self.birthday_text.trim(), "%Y-%m-%d").ok()
        };

        contact.categories = self
            .categories_text
            .split(',')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect();

        // Blank rows are what an "Add" button that was clicked once too often
        // leaves behind; writing them out would put empty EMAIL lines on the
        // card and sync them everywhere.
        contact.emails.retain(|e| !e.value.trim().is_empty());
        contact.phones.retain(|p| !p.value.trim().is_empty());
        contact.urls.retain(|u| !u.value.trim().is_empty());
        contact.nicknames.retain(|n| !n.trim().is_empty());
        contact.addresses.retain(|a| !a.is_empty());

        contact
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Text(field, value) => self.set_text(field, value),
            Message::ListValue(kind, index, value) => match kind {
                ListKind::Nickname => {
                    if let Some(slot) = self.contact.nicknames.get_mut(index) {
                        *slot = value;
                    }
                }
                _ => {
                    if let Some(entry) = list_mut(&mut self.contact, kind).get_mut(index) {
                        entry.value = value;
                    }
                }
            },
            Message::ListLabel(kind, index, label) => {
                let chosen = LABELS.get(label).map(|s| (*s).to_owned());
                if let Some(entry) = list_mut(&mut self.contact, kind).get_mut(index) {
                    // Replace the leading type and keep any others the card
                    // carried (`work,voice` keeps `voice`).
                    let rest: Vec<String> = entry.types.iter().skip(1).cloned().collect();
                    entry.types = chosen.into_iter().chain(rest).collect();
                }
            }
            Message::ListPreferred(kind, index) => {
                // PREF is a ranking, and exactly one first choice is the only
                // ranking a checkbox can express — so setting one clears the
                // rest rather than leaving two values both claiming PREF=1.
                let list = list_mut(&mut self.contact, kind);
                let already = list.get(index).and_then(|e| e.pref) == Some(1);
                for entry in list.iter_mut() {
                    entry.pref = None;
                }
                if !already && let Some(entry) = list.get_mut(index) {
                    entry.pref = Some(1);
                }
            }
            Message::ListRemove(kind, index) => match kind {
                ListKind::Nickname => {
                    if index < self.contact.nicknames.len() {
                        self.contact.nicknames.remove(index);
                    }
                }
                _ => {
                    let list = list_mut(&mut self.contact, kind);
                    // Grouped entries have no remove button; guard anyway, so a
                    // stale message from a re-render cannot delete a row the
                    // patcher would then silently restore.
                    if list.get(index).is_some_and(|e| !e.is_grouped()) {
                        list.remove(index);
                    }
                }
            },
            Message::ListAdd(kind) => match kind {
                ListKind::Nickname => self.contact.nicknames.push(String::new()),
                _ => {
                    let default_type = match kind {
                        ListKind::Phone => "mobile",
                        _ => "home",
                    };
                    let mut entry = Typed::new(String::new());
                    entry.types = vec![default_type.to_owned()];
                    list_mut(&mut self.contact, kind).push(entry);
                }
            },
            Message::AddressPart(index, part, value) => {
                if let Some(address) = self.contact.addresses.get_mut(index) {
                    let slot = match part {
                        AddressPart::Street => &mut address.street,
                        AddressPart::Extended => &mut address.extended,
                        AddressPart::Locality => &mut address.locality,
                        AddressPart::Region => &mut address.region,
                        AddressPart::PostalCode => &mut address.postal_code,
                        AddressPart::Country => &mut address.country,
                    };
                    *slot = value;
                }
            }
            Message::AddressRemove(index) => {
                if index < self.contact.addresses.len() {
                    self.contact.addresses.remove(index);
                }
            }
            Message::AddressAdd => self.contact.addresses.push(Address {
                types: vec!["home".to_owned()],
                ..Address::default()
            }),
            Message::Book(index) => {
                if let Some(id) = self.books.get(index) {
                    self.contact.addressbook_id.clone_from(id);
                }
            }
            Message::GroupToggled(index, member) => {
                if let Some(row) = self.groups.get_mut(index) {
                    row.member = member;
                }
            }
            // Handled by the shell; nothing to record until the answer comes.
            Message::PhotoPickRequested => {}
            Message::PhotoChosen(path) => self.photo = PhotoEdit::Set(path),
            Message::PhotoRemove => self.photo = PhotoEdit::Remove,
            Message::PhotoKeep => self.photo = PhotoEdit::Keep,
        }
    }

    fn set_text(&mut self, field: Field, value: String) {
        match field {
            Field::DisplayName => self.contact.display_name = value,
            Field::Given => self.contact.name.given = value,
            Field::Family => self.contact.name.family = value,
            Field::Additional => self.contact.name.additional = value,
            Field::Prefix => self.contact.name.prefix = value,
            Field::Suffix => self.contact.name.suffix = value,
            // An emptied optional field becomes `None`, not `Some("")`: the
            // patcher removes a property whose edit yields no lines, and an
            // empty `ORG:` left on the card is not the same as no ORG at all.
            Field::Organisation => self.contact.organisation = non_empty(value),
            Field::JobTitle => self.contact.title = non_empty(value),
            Field::Note => self.contact.note = non_empty(value),
            Field::Birthday => self.birthday_text = value,
            Field::Categories => self.categories_text = value,
        }
    }
}

fn non_empty(value: String) -> Option<String> {
    (!value.trim().is_empty()).then_some(value)
}

fn writable(books: &[CalendarMeta]) -> (Vec<String>, Vec<String>) {
    books
        .iter()
        .filter(|b| !b.read_only)
        .map(|b| (b.id.clone(), b.name.clone()))
        .unzip()
}

fn list_mut(contact: &mut Contact, kind: ListKind) -> &mut Vec<Typed> {
    match kind {
        ListKind::Email => &mut contact.emails,
        ListKind::Phone => &mut contact.phones,
        ListKind::Url => &mut contact.urls,
        // Nicknames are `Vec<String>` and are handled by their callers before
        // reaching here; this arm keeps the borrow checker happy without a
        // panic path in a UI event handler.
        ListKind::Nickname => &mut contact.urls,
    }
}

/* ------------------------------------------------------------------ */
/* View                                                               */

/// The editor pane.
pub fn view(state: &State) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut column = widget::column::with_capacity(8).spacing(spacing.space_m);

    column = column.push(name_section(state));

    if state.is_new && state.book_names.len() > 1 {
        let selected = state
            .books
            .iter()
            .position(|id| *id == state.contact.addressbook_id);
        column = column.push(widget::settings::section().title(fl!("in-book")).add(
            widget::settings::item::builder(fl!("address-books")).control(widget::dropdown(
                &state.book_names,
                selected,
                Message::Book,
            )),
        ));
    }

    column = column.push(typed_section(
        fl!("email"),
        ListKind::Email,
        &state.contact.emails,
        fl!("add-email"),
    ));
    column = column.push(typed_section(
        fl!("phone"),
        ListKind::Phone,
        &state.contact.phones,
        fl!("add-phone"),
    ));
    column = column.push(address_section(state));
    column = column.push(typed_section(
        fl!("website"),
        ListKind::Url,
        &state.contact.urls,
        fl!("add-url"),
    ));
    column = column.push(nickname_section(state));
    column = column.push(photo_section(state));
    column = column.push(groups_section(state));
    column = column.push(other_section(state));

    widget::scrollable(column.padding(spacing.space_s))
        .height(Length::Fill)
        .into()
}

/// One labelled text field bound to a single-value model field.
fn text_row<'a>(label: String, value: &'a str, field: Field) -> Element<'a, Message> {
    widget::settings::item::builder(label)
        .control(
            widget::text_input("", value)
                .on_input(move |v| Message::Text(field, v))
                .width(Length::Fill),
        )
        .into()
}

fn name_section(state: &State) -> Element<'_, Message> {
    let c = &state.contact;
    let text = text_row;

    widget::settings::section()
        .title(fl!("edit-contact"))
        .add(text(fl!("name-given"), &c.name.given, Field::Given))
        .add(text(fl!("name-family"), &c.name.family, Field::Family))
        .add(text(
            fl!("name-additional"),
            &c.name.additional,
            Field::Additional,
        ))
        .add(text(fl!("name-prefix"), &c.name.prefix, Field::Prefix))
        .add(text(fl!("name-suffix"), &c.name.suffix, Field::Suffix))
        .add(
            widget::settings::item::builder(fl!("display-name"))
                .description(fl!("display-name-description"))
                .control(
                    widget::text_input(c.name.joined(), &c.display_name)
                        .on_input(|v| Message::Text(Field::DisplayName, v))
                        .width(Length::Fill),
                ),
        )
        .add(text(
            fl!("organisation"),
            c.organisation.as_deref().unwrap_or_default(),
            Field::Organisation,
        ))
        .add(text(
            fl!("title"),
            c.title.as_deref().unwrap_or_default(),
            Field::JobTitle,
        ))
        .add(
            widget::settings::item::builder(fl!("birthday"))
                .description(fl!("birthday-format"))
                .control(
                    widget::text_input("1970-01-01", &state.birthday_text)
                        .on_input(|v| Message::Text(Field::Birthday, v))
                        .width(Length::Fill),
                ),
        )
        .add(
            widget::settings::item::builder(fl!("categories"))
                .description(fl!("categories-hint"))
                .control(
                    widget::text_input("", &state.categories_text)
                        .on_input(|v| Message::Text(Field::Categories, v))
                        .width(Length::Fill),
                ),
        )
        .add(
            widget::settings::item::builder(fl!("note")).control(
                widget::text_input("", c.note.as_deref().unwrap_or_default())
                    .on_input(|v| Message::Text(Field::Note, v))
                    .width(Length::Fill),
            ),
        )
        .into()
}

/// One repeating section of typed values — emails, phones, or websites.
fn typed_section(
    title: String,
    kind: ListKind,
    values: &[Typed],
    add_label: String,
) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut section = widget::settings::section().title(title);

    for (index, entry) in values.iter().enumerate() {
        let value_input = widget::text_input("", &entry.value)
            .on_input(move |v| Message::ListValue(kind, index, v))
            .width(Length::Fill);

        let mut row = widget::row::with_capacity(4)
            .align_y(cosmic::iced::Alignment::Center)
            .spacing(spacing.space_xs)
            .push(value_input);

        if entry.is_grouped() {
            // The label lives in a sibling `X-ABLabel` line this editor does
            // not own. Show it, do not pretend it is editable, and offer no
            // remove — see the module docs.
            row = row.push(
                widget::text::caption(entry.label().unwrap_or_default().to_owned())
                    .class(cosmic::theme::Text::Custom(super::dim_text)),
            );
        } else {
            let selected = entry
                .types
                .first()
                .and_then(|t| LABELS.iter().position(|l| l.eq_ignore_ascii_case(t)));
            row = row
                .push(widget::dropdown(LABELS, selected, move |l| {
                    Message::ListLabel(kind, index, l)
                }))
                .push(preferred_button(kind, index, entry.pref == Some(1)))
                .push(widget::tooltip(
                    widget::button::icon(crate::ui::icon("list-remove-symbolic"))
                        .on_press(Message::ListRemove(kind, index)),
                    widget::text::body(fl!("remove")),
                    widget::tooltip::Position::Top,
                ));
        }

        section = section.add(row);
    }

    section = section.add(widget::button::text(add_label).on_press(Message::ListAdd(kind)));
    section.into()
}

/// The PREF star for one entry.
fn preferred_button<'a>(kind: ListKind, index: usize, is_preferred: bool) -> Element<'a, Message> {
    let icon = if is_preferred {
        "starred-symbolic"
    } else {
        "non-starred-symbolic"
    };
    widget::tooltip(
        widget::button::icon(crate::ui::icon(icon)).on_press(Message::ListPreferred(kind, index)),
        widget::text::body(fl!("set-preferred")),
        widget::tooltip::Position::Top,
    )
    .into()
}

fn nickname_section(state: &State) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut section = widget::settings::section().title(fl!("nickname"));

    for (index, nickname) in state.contact.nicknames.iter().enumerate() {
        section = section.add(
            widget::row::with_capacity(2)
                .align_y(cosmic::iced::Alignment::Center)
                .spacing(spacing.space_xs)
                .push(
                    widget::text_input("", nickname)
                        .on_input(move |v| Message::ListValue(ListKind::Nickname, index, v))
                        .width(Length::Fill),
                )
                .push(widget::tooltip(
                    widget::button::icon(crate::ui::icon("list-remove-symbolic"))
                        .on_press(Message::ListRemove(ListKind::Nickname, index)),
                    widget::text::body(fl!("remove")),
                    widget::tooltip::Position::Top,
                )),
        );
    }

    section
        .add(
            widget::button::text(fl!("add-nickname"))
                .on_press(Message::ListAdd(ListKind::Nickname)),
        )
        .into()
}

/// One component of a structured address.
fn address_row<'a>(
    label: String,
    value: &'a str,
    index: usize,
    which: AddressPart,
) -> Element<'a, Message> {
    widget::settings::item::builder(label)
        .control(
            widget::text_input("", value)
                .on_input(move |v| Message::AddressPart(index, which, v))
                .width(Length::Fill),
        )
        .into()
}

fn address_section(state: &State) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();
    let mut section = widget::settings::section().title(fl!("address"));

    for (index, address) in state.contact.addresses.iter().enumerate() {
        let block = widget::column::with_capacity(7)
            .spacing(spacing.space_xxs)
            .push(address_row(
                fl!("address-street"),
                &address.street,
                index,
                AddressPart::Street,
            ))
            .push(address_row(
                fl!("address-extended"),
                &address.extended,
                index,
                AddressPart::Extended,
            ))
            .push(address_row(
                fl!("address-locality"),
                &address.locality,
                index,
                AddressPart::Locality,
            ))
            .push(address_row(
                fl!("address-region"),
                &address.region,
                index,
                AddressPart::Region,
            ))
            .push(address_row(
                fl!("address-postcode"),
                &address.postal_code,
                index,
                AddressPart::PostalCode,
            ))
            .push(address_row(
                fl!("address-country"),
                &address.country,
                index,
                AddressPart::Country,
            ))
            .push(widget::button::text(fl!("remove")).on_press(Message::AddressRemove(index)));

        section = section.add(block);
    }

    section
        .add(widget::button::text(fl!("add-address")).on_press(Message::AddressAdd))
        .into()
}

/// The photo controls: set, remove, undo.
///
/// The file dialog itself lives in the shell (it is async); this section only
/// records the intent and says plainly what will happen on save.
fn photo_section(state: &State) -> Element<'_, Message> {
    let spacing = cosmic::theme::spacing();

    let status = match &state.photo {
        PhotoEdit::Keep if state.contact.has_photo => fl!("photo-current"),
        PhotoEdit::Keep => fl!("photo-none"),
        PhotoEdit::Set(path) => fl!(
            "photo-pending",
            name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        ),
        PhotoEdit::Remove => fl!("photo-removing"),
    };

    let mut controls = widget::row::with_capacity(4)
        .align_y(cosmic::iced::Alignment::Center)
        .spacing(spacing.space_xs)
        .push(widget::text::caption(status).class(cosmic::theme::Text::Custom(super::dim_text)))
        .push(widget::button::standard(fl!("set-photo")).on_press(Message::PhotoPickRequested));

    if state.photo != PhotoEdit::Keep {
        controls =
            controls.push(widget::button::standard(fl!("undo")).on_press(Message::PhotoKeep));
    }
    if state.contact.has_photo && state.photo == PhotoEdit::Keep {
        controls =
            controls.push(widget::button::standard(fl!("remove")).on_press(Message::PhotoRemove));
    }

    widget::settings::section()
        .title(fl!("photo"))
        .add(widget::settings::item::builder(fl!("photo")).control(controls))
        .into()
}

/// Membership togglers, one per group card in the contact's book.
///
/// Absent entirely when the book has no groups: an empty section headed
/// "In groups" would only prompt "how do I make one?" with no answer here —
/// the File menu owns creation.
fn groups_section(state: &State) -> Element<'_, Message> {
    if state.groups.is_empty() {
        return widget::column::with_capacity(0).into();
    }

    let mut section = widget::settings::section().title(fl!("in-groups"));
    for (index, row) in state.groups.iter().enumerate() {
        section = section.add(
            widget::settings::item::builder(row.name.clone()).toggler(row.member, move |member| {
                Message::GroupToggled(index, member)
            }),
        );
    }
    section.into()
}

/// The read-only list of properties the card carries that this editor will not
/// change — the honesty section 03-circle.md asks for.
fn other_section(state: &State) -> Element<'_, Message> {
    let names = unmodelled_properties(&state.contact.raw);
    if names.is_empty() {
        return widget::column::with_capacity(0).into();
    }

    let spacing = cosmic::theme::spacing();
    widget::settings::section()
        .title(fl!("other-fields"))
        .add(
            widget::column::with_capacity(2)
                .spacing(spacing.space_xxs)
                .push(
                    widget::text::caption(fl!("other-fields-description"))
                        .class(cosmic::theme::Text::Custom(super::dim_text)),
                )
                .push(widget::text::body(names.join(", "))),
        )
        .into()
}

/// Property names present in a stored card that [`Contact`] does not model.
///
/// Read off the source text rather than a list of "things we know about", so a
/// property nobody anticipated still shows up here instead of being invisible.
#[must_use]
pub fn unmodelled_properties(raw: &str) -> Vec<String> {
    use cosmic_pim_core::patch::logical_lines;

    /// Everything the editor above writes, plus the structural and versioning
    /// properties that are not user data.
    const MODELLED: &[&str] = &[
        "BEGIN",
        "END",
        "VERSION",
        "UID",
        "REV",
        "FN",
        "N",
        "NICKNAME",
        "EMAIL",
        "TEL",
        "ADR",
        "ORG",
        "TITLE",
        "NOTE",
        "BDAY",
        "CATEGORIES",
        "PRODID",
    ];

    let mut seen: Vec<String> = Vec::new();
    for line in logical_lines(raw) {
        // `name()` is uppercased, which is what a case-insensitive comparison
        // against `MODELLED` wants and what a reader does not: `X-ABSHOWAS` is
        // shouting, and the card says `X-ABShowAs`.
        let canonical = line.name();
        if canonical.is_empty() || MODELLED.contains(&canonical.as_str()) {
            continue;
        }
        let display = source_name(line.unfolded());
        if !display.is_empty() && !seen.contains(&display) {
            seen.push(display);
        }
    }
    seen
}

/// A property name exactly as the card spells it, with any group prefix removed
/// (`item1.X-ABLabel` → `X-ABLabel`).
fn source_name(unfolded: &str) -> String {
    let end = unfolded.find([';', ':']).unwrap_or(unfolded.len());
    let name = unfolded[..end].trim();
    match name.split_once('.') {
        Some((_group, rest)) => rest.to_owned(),
        None => name.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SYNCED: &str = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:ada@server\r\n\
FN:Ada Lovelace\r\nEMAIL;TYPE=work:ada@work.example\r\n\
item1.EMAIL;type=INTERNET:ada@home.example\r\nitem1.X-ABLabel:Summer house\r\n\
PHOTO;ENCODING=b:AAAABBBB\r\nX-ABShowAs:COMPANY\r\nGEO:geo:37.98,23.72\r\nEND:VCARD\r\n";

    fn state() -> State {
        State::create("default", &[])
    }

    #[test]
    fn an_emptied_optional_field_becomes_none_rather_than_an_empty_string() {
        let mut state = state();
        state.update(Message::Text(Field::Organisation, "Acme".into()));
        assert_eq!(state.contact.organisation.as_deref(), Some("Acme"));

        state.update(Message::Text(Field::Organisation, String::new()));
        assert!(
            state.contact.organisation.is_none(),
            "an empty ORG would be written to the card as a blank property"
        );
    }

    #[test]
    fn marking_one_value_preferred_clears_the_others() {
        let mut state = state();
        state.update(Message::ListAdd(ListKind::Email));
        state.update(Message::ListAdd(ListKind::Email));
        state.update(Message::ListPreferred(ListKind::Email, 0));
        state.update(Message::ListPreferred(ListKind::Email, 1));

        assert_eq!(state.contact.emails[0].pref, None);
        assert_eq!(state.contact.emails[1].pref, Some(1));
    }

    #[test]
    fn clicking_preferred_twice_turns_it_off() {
        let mut state = state();
        state.update(Message::ListAdd(ListKind::Email));
        state.update(Message::ListPreferred(ListKind::Email, 0));
        state.update(Message::ListPreferred(ListKind::Email, 0));
        assert_eq!(state.contact.emails[0].pref, None);
    }

    #[test]
    fn changing_a_label_keeps_the_other_types_on_the_line() {
        let mut state = state();
        state.contact.phones.push(Typed {
            value: "+30 210 0000000".into(),
            types: vec!["work".into(), "voice".into()],
            pref: None,
            group: None,
            params: Vec::new(),
        });

        state.update(Message::ListLabel(ListKind::Phone, 0, 2)); // "mobile"
        assert_eq!(state.contact.phones[0].types, vec!["mobile", "voice"]);
    }

    /// Dropping a grouped entry from the list would not remove it from the
    /// card, so the control does not exist and the handler refuses too.
    #[test]
    fn a_grouped_entry_cannot_be_removed() {
        let mut state = state();
        state.contact.emails.push(Typed {
            value: "ada@home.example".into(),
            types: vec!["internet".into()],
            pref: None,
            group: Some("item1".into()),
            params: Vec::new(),
        });

        state.update(Message::ListRemove(ListKind::Email, 0));
        assert_eq!(
            state.contact.emails.len(),
            1,
            "a grouped entry was dropped from the model but would survive on disk"
        );
    }

    #[test]
    fn blank_rows_are_dropped_on_save() {
        let mut state = state();
        state.contact.display_name = "Ada".into();
        state.update(Message::ListAdd(ListKind::Email));
        state.update(Message::ListAdd(ListKind::Phone));
        state.update(Message::AddressAdd);

        let finished = state.finish();
        assert!(finished.emails.is_empty());
        assert!(finished.phones.is_empty());
        assert!(
            finished.addresses.is_empty(),
            "an address with only a TYPE would be written as a blank ADR line"
        );
    }

    #[test]
    fn categories_are_split_on_save_not_on_every_keystroke() {
        let mut state = state();
        state.contact.display_name = "Ada".into();
        state.update(Message::Text(Field::Categories, "Friends, Work ,".into()));

        assert!(state.contact.categories.is_empty(), "split too early");
        assert_eq!(state.finish().categories, vec!["Friends", "Work"]);
    }

    #[test]
    fn a_birthday_parses_on_save_and_a_mistyped_one_does_not_block_it() {
        let mut state = state();
        state.contact.display_name = "Ada".into();

        state.update(Message::Text(Field::Birthday, "1815-12-10".into()));
        assert_eq!(
            state.finish().birthday,
            chrono::NaiveDate::from_ymd_opt(1815, 12, 10)
        );

        state.update(Message::Text(Field::Birthday, "not a date".into()));
        let finished = state.finish();
        assert!(finished.birthday.is_none());
        assert_eq!(finished.label(), "Ada", "the rest of the edit was lost");
    }

    #[test]
    fn a_contact_with_no_name_at_all_is_not_saveable() {
        let mut state = state();
        assert!(!state.is_saveable());

        state.update(Message::Text(Field::Given, "Ada".into()));
        assert!(state.is_saveable());
    }

    #[test]
    fn an_email_alone_is_enough_to_save() {
        let mut state = state();
        state.update(Message::ListAdd(ListKind::Email));
        state.update(Message::ListValue(
            ListKind::Email,
            0,
            "ada@example.com".into(),
        ));
        assert!(state.is_saveable());
    }

    #[test]
    fn unmodelled_properties_are_listed_and_modelled_ones_are_not() {
        let names = unmodelled_properties(SYNCED);
        assert!(names.contains(&"PHOTO".to_owned()), "{names:?}");
        assert!(names.contains(&"X-ABShowAs".to_owned()), "{names:?}");
        assert!(
            names.contains(&"X-ABLabel".to_owned()),
            "a grouped label the editor cannot change was not disclosed: {names:?}"
        );
        assert!(names.contains(&"GEO".to_owned()), "{names:?}");
        assert!(!names.contains(&"FN".to_owned()), "{names:?}");
        assert!(!names.contains(&"EMAIL".to_owned()), "{names:?}");
        assert!(!names.contains(&"BEGIN".to_owned()), "{names:?}");
    }

    /// Names are shown as the card spells them, not shouted.
    #[test]
    fn property_names_keep_their_source_casing_and_lose_their_group() {
        let names = unmodelled_properties(SYNCED);
        assert!(!names.iter().any(|n| n == "X-ABSHOWAS"), "{names:?}");
        assert!(!names.iter().any(|n| n.contains('.')), "{names:?}");
    }

    #[test]
    fn a_card_with_nothing_unmodelled_lists_nothing() {
        let plain = "BEGIN:VCARD\r\nVERSION:4.0\r\nUID:x\r\nFN:Ada\r\nEND:VCARD\r\n";
        assert!(unmodelled_properties(plain).is_empty());
    }

    #[test]
    fn editing_a_grouped_value_still_works() {
        let mut state = state();
        state.contact.emails.push(Typed {
            value: "ada@home.example".into(),
            types: vec!["internet".into()],
            pref: None,
            group: Some("item1".into()),
            params: Vec::new(),
        });

        state.update(Message::ListValue(
            ListKind::Email,
            0,
            "new@home.example".into(),
        ));
        assert_eq!(state.contact.emails[0].value, "new@home.example");
        assert_eq!(
            state.contact.emails[0].group.as_deref(),
            Some("item1"),
            "the group was lost, which would orphan the custom label"
        );
    }
}
