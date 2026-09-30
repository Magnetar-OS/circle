// SPDX-License-Identifier: GPL-3.0-only

//! What the sidebar and the list show: the nav entries, the rows the
//! current filter and query leave, and who is selected among them.

use cosmic::app::Task;
use cosmic::prelude::*;
use cosmic::widget::{self, nav_bar};
use cosmic_pim_core::model::{CalendarMeta, Contact};

use super::{AppModel, COLLAPSE_WIDTH, ContactKey, Message, NavEntry};
use crate::fl;
use crate::ui::editor;

impl AppModel {
    /// Updates the header and window titles.
    pub(super) fn update_title(&mut self) -> Task<Message> {
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
    pub(super) fn rebuild_nav(&mut self) {
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
        if self.crm.has_any_cadence() {
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
    pub(super) fn reload(&mut self) {
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

        // Starred people lead the list, each half in the order it was just
        // given. Last, so nothing after it can reorder the rows it counted.
        self.favorites = crate::crm::favorites_first(&mut self.contacts, &self.crm, &self.links);

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

    /// Whether the row at `key` is in the list's starred section.
    pub(super) fn is_favorite(&self, key: &ContactKey) -> bool {
        self.contacts
            .iter()
            .take(self.favorites)
            .any(|contact| key.matches(contact))
    }

    /// Stars or unstars the person on the row at `key` — all of their cards,
    /// so the star does not depend on which of them the row stands on.
    pub(super) fn set_favorite(&mut self, key: &ContactKey, favorite: bool) -> Task<Message> {
        let mut cards = self.links.cards_of_person(&key.book, &key.uid);
        // A link record goes on naming a card after it is deleted. Starring
        // that one would write a record about nobody.
        if favorite
            && cards.len() > 1
            && let Some(store) = self.store.as_ref()
        {
            cards.retain(|card| store.contact(&card.book, &card.uid).is_some());
        }
        let result = self.crm.set_favorite(&cards, favorite);
        // Even when it failed: part of it may have been written.
        self.reload();
        match result {
            Ok(()) => Task::none(),
            Err(why) => self.toast(why),
        }
    }

    /// Collapses every linked person in the list down to one row.
    ///
    /// The row that survives is the person's precedence head — the first card
    /// in the link record that the current filter left visible, so filtering
    /// to one book still shows that book's card rather than nothing. The
    /// others move to `folded`, where the detail pane composes them back in.
    pub(super) fn fold_links(&mut self) {
        self.folded.clear();
        let folded = self.links.fold(std::mem::take(&mut self.contacts));
        self.contacts = folded.rows;
        self.folded = folded
            .members
            .into_iter()
            .map(|(head, cards)| {
                (
                    ContactKey {
                        book: head.book,
                        uid: head.uid,
                    },
                    cards,
                )
            })
            .collect();

        // A selection that just became a folded member follows its head,
        // rather than leaving the detail pane empty.
        if let Some(selected) = self.selected.clone()
            && !self.contacts.iter().any(|c| selected.matches(c))
            && let Some(head) = self
                .folded
                .iter()
                .find(|(_, cards)| cards.iter().any(|c| selected.matches(c)))
                .map(|(head, _)| head.clone())
        {
            self.selected = Some(head);
        }
    }

    /// The cards behind the selected row, head first, each with its book's
    /// display name — what [`crate::ui::person::compose`] reads.
    pub(super) fn selected_cards(&self) -> Vec<(&Contact, &str)> {
        match self.selected.as_ref() {
            Some(key) => self.cards_for(key),
            None => Vec::new(),
        }
    }

    /// [`Self::selected_cards`] for any row, not just the selected one.
    pub(super) fn cards_for(&self, key: &ContactKey) -> Vec<(&Contact, &str)> {
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

    pub(super) fn selected_contact(&self) -> Option<&Contact> {
        let key = self.selected.as_ref()?;
        self.contacts.iter().find(|c| key.matches(c))
    }

    /// Re-resolves the selected person's relationships.
    ///
    /// Against every contact in a visible book, not the filtered list: a
    /// relationship to somebody the current search hides is still a
    /// relationship, and one that silently stopped resolving when you typed
    /// would look like the card had changed.
    pub(super) fn refresh_relations(&mut self) {
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
    pub(super) fn person_cards(&self) -> Vec<crate::links::CardRef> {
        self.selected_cards()
            .into_iter()
            .map(|(contact, _)| crate::links::CardRef {
                book: contact.addressbook_id.clone(),
                uid: contact.uid.clone(),
            })
            .collect()
    }

    pub(super) fn head_card(&self) -> Option<crate::links::CardRef> {
        self.person_cards().into_iter().next()
    }

    /// Everybody past their cadence, as of now.
    ///
    /// Recomputed rather than cached: it depends on the clock, so a cached
    /// answer is wrong by definition the moment it is stored.
    pub(super) fn overdue_keys(&self) -> Vec<ContactKey> {
        if !self.crm.has_any_cadence() {
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
    pub(super) fn cards_of(&self, key: &ContactKey) -> Vec<crate::links::CardRef> {
        self.cards_for(key)
            .into_iter()
            .map(|(contact, _)| crate::links::CardRef {
                book: contact.addressbook_id.clone(),
                uid: contact.uid.clone(),
            })
            .collect()
    }

    /// Whether the window is too narrow for panes side by side.
    pub(super) fn is_collapsed(&self) -> bool {
        self.width < COLLAPSE_WIDTH
    }

    /// Ticks every row between the selection anchor and `key`, inclusive, in
    /// the list's current order — what Shift+click means everywhere else.
    pub(super) fn check_range_to(&mut self, key: &ContactKey) {
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

    pub(super) fn writable_books(&self) -> Vec<CalendarMeta> {
        self.store
            .as_ref()
            .map(|s| s.books().iter().filter(|b| !b.read_only).cloned().collect())
            .unwrap_or_default()
    }

    pub(super) fn rebuild_writable(&mut self) {
        let books = self.writable_books();
        self.writable_ids = books.iter().map(|b| b.id.clone()).collect();
        self.writable_names = books.iter().map(|b| b.name.clone()).collect();
    }

    /// The membership rows for the editor: every group card in `book`, marked
    /// with whether `uid` is currently a member.
    pub(super) fn group_rows(&self, book: &str, uid: Option<&str>) -> Vec<editor::GroupRow> {
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
}
