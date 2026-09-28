# Circle — feature parity audit

Per the roadmap's benchmark table: **baseline** GNOME Contacts (must fully
cover), **ceiling** GNOME Contacts + Monica's CRM layer, **polish reference**
Apple Contacts (how it should feel, not what it does — excluded from this
audit). Method: GNOME Contacts' surface from apps.gnome.org, the GNOME help
pages, and release notes through GNOME 51 (Contacts 51.0, 13 September
2026);
Monica's from monicahq.com (v2 shipped features; v3's custom-records design
noted where relevant). Circle's side from README.md, 03-circle.md, and the
source where a row was uncertain. Status values: **have** / **partial** /
**gap** / **rejected** (with the reason from 03-circle.md's non-goals) /
**verify** where honesty requires checking rather than guessing.

Audited 2026-08-27; re-audited 2026-09-29 against the code at that date
and against GNOME Contacts 51.0. Every **have** below was checked in the
source, not in the docs.

**What 51.0 changed.** Read from the 50.0…51.0 comparison on
gitlab.gnome.org (the release has no NEWS body): contact photos persist in
the Flatpak build after a restart, import errors are announced to screen
readers, and translations. No feature was added or removed, so the baseline
rows are unchanged; the import-error announcement is recorded under
accessibility.

## Baseline: GNOME Contacts

### List and detail

| Feature | Status | Notes |
|---|---|---|
| Contact list with avatars | have | Inline PHOTO decoded; initials on a name-seeded colour otherwise. |
| Detail pane | have | Every value selectable with a copy button. |
| Sort by first or last name | have | Settings, via cosmic-config. |
| Adaptive layout (desktop → narrow) | have | Three panes wide; list/detail take turns below 640 px, down to 360 px. |
| Selection mode (multi-select operations) | have | Select button, Ctrl+click, Shift+range, Ctrl+A; delete, export, add-to-group. |
| Favorites pinned to top of list | gap | GNOME Contacts marks favorites; Circle has no equivalent (the star in the editor is the PREF toggle, a different thing). |
| mailto:/tel: actions from the detail pane | have | Through the desktop handler; KDE Connect picks up `tel:` when installed. Direct D-Bus handoff open (03 §3). |
| Address opens in a maps app | verify | GNOME's behaviour and Circle's both unchecked; Circle shows the address as text. |
| Share contact as QR code | have | `ui/share.rs`: a trimmed card (no photo) fitted to one code, SVG rendered in the app. |

### Editing

| Feature | Status | Notes |
|---|---|---|
| Create and delete contacts | have | Single delete is immediate with byte-identical Undo; batch delete confirms first. |
| Field editors: name, email, phone, address, org/title, birthday, nickname, website, notes | have | With TYPE labels and one PREF per list. |
| Categories/tags on a contact | have | Editor field; GNOME Contacts has no categories UI at all — Circle exceeds baseline here. |
| IM handles (IMPP) editing | partial | Preserved byte-for-byte by the patcher, listed in the "other fields" honesty section, not editable. Verify whether current GNOME Contacts still edits IM at all. |
| Custom (Apple-grouped) labels | partial | Shown as text, value editable, deliberately not removable — dropping one from the model would not remove it from the card. GNOME does not handle these at all. |
| Round-trip safety of unmodeled properties | have | Edits patch the stored bytes; PHOTO, GEO, X- properties, grouped labels survive. Pinned by tests/write_path.rs. GNOME (via EDS) re-serialises. |
| vCard version discipline | have | New cards 3.0 (4.0 by setting); existing cards keep their declared dialect, never converted silently. |

### Photos

| Feature | Status | Notes |
|---|---|---|
| Display inline photos (3.0 `ENCODING=b` and 4.0 `data:`) | have | Decoded once per selection. |
| Set / replace / remove photo | have | Center-cropped square, scaled to 512 px, patched in the card's own dialect. |
| Generated initials avatars | have | |
| Fetch remote photo URIs | rejected | No network for avatars, by design; URI-form photos surfaced as URIs, never fetched. |

### Groups

GNOME Contacts has no group UI; every row here exceeds the baseline and
counts against the ceiling.

| Feature | Status | Notes |
|---|---|---|
| CATEGORIES groups: sidebar filter, chips, assignment | have | Groups read live off the cards. |
| KIND:group cards, both spellings | have | 4.0 `KIND:group` and Apple's `X-ADDRESSBOOKSERVER-KIND`; create/delete from File menu, per-group membership toggle in the editor; `set_members` writes each card's own member spelling, only changed groups rewritten. |
| Drag-to-assign | gap | Open in 03 §4. |
| Group as compose list | gap | Waits on Envelope. |

### Linking and duplicates

| Feature | Status | Notes |
|---|---|---|
| Link contacts across accounts/sources | have | `links.rs`: a person is a JSON record under `.links/`; cards are never rewritten; the list folds a person into one row and the detail pane unions their values with per-value book attribution. The launcher folds them too. |
| Unlink | have | Lossless; a person left with one card dissolves. Pinned by `tests/linking.rs`. |
| Automatic linking of matching contacts | rejected | GNOME auto-links same-name contacts. Circle suggests (duplicate review) and never links on its own: a wrong automatic link silently mixes two people's values in every view. |
| Duplicate review (candidates, side-by-side diff) | have | `dedupe.rs`: shared email, shared number by significant trailing digits (no invented country code), transliteration-aware name similarity; link or "not the same person", remembered. |
| Automatic merging | rejected | Non-goal: no auto-merge, ever. Destructive merge will exist only as a separate, explicit, undoable operation. |

### Import and export

| Feature | Status | Notes |
|---|---|---|
| vCard import | have | Through the file portal; UID-keyed, re-import updates rather than duplicates; multi-card files split verbatim. |
| vCard export | have | Stored bytes verbatim, per book or all visible books. |
| Open `.vcf` from a file manager | have | Real MimeType registration; hands over to the running instance. |
| CSV import with column mapping | have | Exceeds baseline — GNOME Contacts has none. Explicit mapping screen, samples per column, mapped UID makes re-import update through the patcher. |

### Sync and accounts

| Feature | Status | Notes |
|---|---|---|
| Local address book | have | Plain vdir; khard/vdirsyncer-compatible. |
| CardDAV accounts (URL, username, password) | have | Suite-shared accounts.toml, password in the keychain; on-demand or background cadence sync (off by default); unbound books stay local. |
| Google contacts via OAuth | partial | The substrate syncs OAuth accounts (provider manifests, `cosmic_pim_accounts::Registry`) and Circle syncs every account in the shared store, but Circle's own add form asks only for URL, user and password; an OAuth account has to be added elsewhere in the suite first. |
| Exchange / EWS contacts | gap | GNOME has it via GOA + EDS. Not in the substrate. |
| External changes noticed live | have | vdirsyncer, khard, or a text editor changing a card updates the list without a restart. |
| Sync conflict resolution UI | have | Accounts page, over `cosmic_pim_sync::conflict`: keep mine, take the server's, merge when the edits touch different fields, or pick a side per disputed field. A pass that leaves one raises a notification. |
| Sync failures visible | have | Per-account status line (including address books that refused the sign-in), and a notification when the set of accounts needing attention changes. |

### Search

| Feature | Status | Notes |
|---|---|---|
| Search names, emails, orgs, nicknames, categories, phones | have | Punctuation-insensitive (`5551234` matches `+1 (555) 123-4`); exceeds GNOME's substring search. |
| Search from outside the app | have | Launcher plugin: `con ada`, Enter opens filtered, context menu copies email/phone or composes, one result per linked person — GNOME has Shell search provider parity here. |

### Accessibility and keyboard

| Feature | Status | Notes |
|---|---|---|
| Keyboard shortcuts via KeyBind table | have | Ctrl+N/E/A/F/I/R, Ctrl+Shift+E, Ctrl+Shift+R (sync), Ctrl+, and Delete (src/key_bind.rs); arrows walk the list. |
| Every action keyboard-reachable | partial | Milestone-5 exit criterion; not audited yet. |
| Shortcut cheat-sheet / palette | gap | The Envelope registry pattern is slated to extend here (roadmap M5). |
| Screen reader, contrast, 125/150% text scaling, reduced motion | verify | Milestone-5 items; current state unmeasured. GNOME 51.0 announces import errors to screen readers; Circle reports them in a toast, whose announcement has not been checked. |
| i18n (Fluent, translated desktop entry/metainfo) | have | Generated at build time from the catalogues; Greek is the proving second locale. vCard TYPE labels and birthday months are translated; a label the catalogue does not know is shown as written. |

## Ceiling: Monica's CRM layer

CRM data is keyed per card and unioned across a linked person, and lives in
`.crm/` beside the books — a dot-directory the collection scanner skips — so
synced books others see stay unpolluted.

| Feature | Status | Notes |
|---|---|---|
| Timestamped notes per person | have | `crm.rs`: JSON per card, written atomically; unioned across a linked person. |
| Activity log ("log interaction") | have | Manual "log contact" with a timestamp; the latest one is the last-contacted date. |
| Automatic last-contacted from mail | gap | Waits on Envelope's sent/received hook (library call, no daemon). |
| Stay-in-touch cadence + overdue list | have | Per-card cadence and a "Keep in touch" sidebar list of who is overdue. No desktop notification: that would be a second scheduler beside Slate's. |
| Birthday reminders | have | In Slate: `cosmic_pim_core::birthdays` synthesises a birthdays calendar from the suite's address books. |
| Relationships between people | partial | `relations.rs` reads vCard 4.0 `RELATED` and Apple's `X-ABRELATEDNAMES`, links the ones that name somebody in the book; read-only — writing needs a substrate patcher for either property. |
| Documents and photos per person | have | `attachments.rs`: content-addressed blobs under `.crm/blobs/`, stored atomically, swept only when every record reads. |
| Syncing those attachments | rejected | Non-goal: attachment sync. |
| Business-card OCR | gap | Explicitly last, behind an optional feature flag. |
| Labels/tags | have | CATEGORIES covers Monica's labels. |
| Journal (free-standing, not per-contact) | rejected | Out of scope for an address book (audit 2026-09-28 R-4). |
| Gifts tracking | rejected | Same. |
| Debts tracking | rejected | Same. |
| Tasks per contact | gap | Undecided: the suite's task owner is Slate, and a link from person to task may be the right shape. Put to the user in the 2026-09-29 fix-run notes. |
| Calls / conversations log | partial | The interaction log records that you were in touch and when; it has no call/conversation type or content. |
| Life events | rejected | Out of scope for an address book (audit 2026-09-28 R-4). |
| API / programmatic access | have | Differently: files-as-truth. Every contact is a plain `.vcf` on disk, every CRM record a plain file — scriptable without an API server. Monica v3's MCP server has no equivalent; record as a rejection if that stands. |
| Custom fields / records-you-design (Monica v3) | partial | X- properties survive by construction and are listed honestly, but there is no UI to define or edit custom fields. |
| Social feeds / profile scraping | rejected | Non-goal: social scraping. |
| Third-party enrichment | rejected | Non-goal: enrichment. |
| LDAP directory (org users) | gap | Planned read-only via the substrate (03 §8), behind the collection abstraction. |
| LDAP write-back | rejected | Non-goal: never written. |

## Data-loss-shaped gaps

The write path patches cards in their own dialect, exports verbatim,
re-imports by UID and undoes deletes byte for byte. The two questions this
section used to leave open are answered:

1. **Concurrent remote edits.** The substrate records a conflict instead of
   overwriting either side, merges automatically when the two edits touch
   different properties and a base was queued (Circle queues one with every
   edit), and parks the rest. Circle shows and resolves them (see Sync
   conflict resolution above).
2. **CSV re-import over an existing contact.** This was a real defect: the
   row replaced the card's modelled fields, so a CSV of name and UID erased
   emails, numbers and addresses and queued that for upload. Fixed on
   2026-09-29: a row now only adds (audit F-01, pinned by
   `tests/write_path.rs`).

Also fixed on 2026-09-29: deleting one card from a multi-card file in a
synced book queued a DELETE of the whole file on the server (audit F-15).

## Ceiling gaps, ranked

1. **Favorites** — the last list-level baseline gap. Needs a storage
   decision first: local (`.crm/`) or synced with the card.
2. **Adding OAuth accounts from Circle** — sync already works; only the form
   is missing.
3. **Exchange / EWS** — not in the substrate.
4. **Editable relationships** — needs a byte-preserving patcher for
   `RELATED` / `X-ABRELATEDNAMES` in the substrate.
5. **Automatic last-contacted from mail** — waits on Envelope's hook.
6. **Tasks per contact** — decide first.
7. **Custom fields UI, drag-to-assign, group as compose list, OCR.**
8. **LDAP read-only** — last, org-user audience, behind the substrate's
   collection abstraction.
