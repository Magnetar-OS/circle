// SPDX-License-Identifier: GPL-3.0-only

//! The window's panes, composed from the pieces in `crate::ui`.

use cosmic::iced::Length;
use cosmic::prelude::*;
use cosmic::widget;

use super::{AppModel, ContactKey, Message, Review, search_id};
use crate::fl;
use crate::ui::{csv, editor};

impl AppModel {
    /// The main window: the list, and whatever claims the pane beside it.
    pub(super) fn main_view(&self) -> Element<'_, Message> {
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
                self.favorites,
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
                            self.can_map,
                            self.is_favorite(&ContactKey::of(person.head)),
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
                            .push(crate::ui::crm::notes(&summary, &self.note_draft, now))
                            .push(crate::ui::crm::attachments(&summary)),
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

    /// The bar under the list while selecting: the count and what can be done
    /// with the set.
    pub(super) fn action_bar(&self) -> Element<'_, Message> {
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

    /// The duplicate review pane, with its own header and close button.
    pub(super) fn review_pane<'a>(&'a self, review: &'a Review) -> Element<'a, Message> {
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
    pub(super) fn side<'a>(
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
    pub(super) fn csv_pane<'a>(&'a self, state: &'a csv::State) -> Element<'a, Message> {
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
    pub(super) fn editor_pane<'a>(&'a self, state: &'a editor::State) -> Element<'a, Message> {
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
