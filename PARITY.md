# Circle — feature parity audit

Per the roadmap's benchmark table: **baseline** GNOME Contacts (must fully
cover), **ceiling** GNOME Contacts + Monica's CRM layer, **polish reference**
Apple Contacts (how it should feel, not what it does — excluded from this
audit). Method: GNOME Contacts' surface from apps.gnome.org, the GNOME help
pages, and release notes through GNOME 50 (Contacts 50.0, March 2026);
Monica's from monicahq.com (v2 shipped features; v3's custom-records design
noted where relevant). Circle's side from README.md, 03-circle.md, and the
source where a row was uncertain. Status values: **have** / **partial** /
**gap** / **rejected** (with the reason from 03-circle.md's non-goals) /
**verify** where honesty requires checking rather than guessing.

Audited 2026-08-27.

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
| Share contact as QR code | gap | GNOME 44+. Nothing in Circle. |

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
| Link contacts across accounts/sources | gap | GNOME's headline aggregation feature. Circle's design (03 §5) is stronger — app-level person, per-field precedence, every edit landing on exactly one card, link store in a local-only collection — but none of it is built. The largest baseline gap. |
| Unlink | gap | Follows the above. |
| Automatic linking of matching contacts | gap | GNOME auto-links same-name contacts across sources. Circle plans candidate *suggestion* only. |
| Duplicate review (candidates, side-by-side diff) | gap | Planned (03 §6): exact email / E.164 phone, transliteration-aware names; default action link. |
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
| Google contacts via OAuth | verify | GNOME gets this through GNOME Online Accounts. Circle's account flow is URL/user/password; whether Google's CardDAV endpoint works with an app password, and whether OAuth is planned in the substrate, needs checking. |
| Exchange / EWS contacts | gap | GNOME has it via GOA + EDS. Not in the substrate. |
| External changes noticed live | have | vdirsyncer, khard, or a text editor changing a card updates the list without a restart. |
| Sync conflict resolution UI | gap | Depends on the substrate's Milestone-1 conflict surfacing API; neither app can build it earlier. See data-loss section. |

### Search

| Feature | Status | Notes |
|---|---|---|
| Search names, emails, orgs, nicknames, categories, phones | have | Punctuation-insensitive (`5551234` matches `+1 (555) 123-4`); exceeds GNOME's substring search. |
| Search from outside the app | have | Launcher plugin: `con ada`, Enter opens filtered, context menu copies email/phone or composes — GNOME has Shell search provider parity here. |

### Accessibility and keyboard

| Feature | Status | Notes |
|---|---|---|
| Keyboard shortcuts via KeyBind table | have | Ctrl+N/E/A/F/I/R, Ctrl+Shift+E, Ctrl+, (src/key_bind.rs). |
| Every action keyboard-reachable | partial | Milestone-5 exit criterion; not audited yet. |
| Shortcut cheat-sheet / palette | gap | The Envelope registry pattern is slated to extend here (roadmap M5). |
| Screen reader, contrast, 125/150% text scaling, reduced motion | verify | Milestone-5 items; current state unmeasured. |
| i18n (Fluent, translated desktop entry/metainfo) | have | Generated at build time from the catalogues; Greek is the proving second locale. |

## Ceiling: Monica's CRM layer

All of this sits behind linking (03 §5) — CRM data attaches to a *person*,
not a card — and lives in the local-only collection so synced books others
see stay unpolluted.

| Feature | Status | Notes |
|---|---|---|
| Timestamped notes per person | gap | Planned (03 §7): plain files in the local-only collection, greppable. |
| Activity log ("log interaction") | gap | Planned as manual last-contacted first. |
| Automatic last-contacted from mail | gap | Waits on Envelope's sent/received hook (library call, no daemon). |
| Stay-in-touch cadence + overdue list | gap | Planned; reuses Slate's reminder machinery once it moves to the substrate — no second scheduler. |
| Birthday reminders | gap | Via substrate BDAY synthesis feeding Slate (roadmap M1); not landed. |
| Relationships between people | gap | Planned as vCard4 RELATED, clickable, text fallback for 3.0 cards. Monica's family/partner/coworker typing would ride on RELATED's type parameter. |
| Documents and photos per person | gap | Planned: local content-addressed blob dir, explicitly local-only in the UI. |
| Syncing those attachments | rejected | Non-goal: attachment sync. |
| Business-card OCR | gap | Explicitly last, behind an optional feature flag. |
| Labels/tags | have | CATEGORIES covers Monica's labels. |
| Journal (free-standing, not per-contact) | gap | Not in 03's plan. Arguably out of scope for a contacts app; needs an explicit have/reject decision so it stops being a hole. |
| Gifts tracking | gap | Not planned. Same: decide, don't leave unlisted. |
| Debts tracking | gap | Not planned. Same. |
| Tasks per contact | gap | Not planned in Circle; the suite's task owner is Slate — a RELATED-style link from person to task may be the right shape. Decide. |
| Calls / conversations log | gap | Not planned as distinct types; the planned interaction log may subsume both. Decide and record. |
| Life events | gap | Not planned. Decide. |
| API / programmatic access | have | Differently: files-as-truth. Every contact is a plain `.vcf` on disk, every CRM record a plain file — scriptable without an API server. Monica v3's MCP server has no equivalent; record as a rejection if that stands. |
| Custom fields / records-you-design (Monica v3) | partial | X- properties survive by construction and are listed honestly, but there is no UI to define or edit custom fields. |
| Social feeds / profile scraping | rejected | Non-goal: social scraping. |
| Third-party enrichment | rejected | Non-goal: enrichment. |
| LDAP directory (org users) | gap | Planned read-only via the substrate (03 §8), behind the collection abstraction. |
| LDAP write-back | rejected | Non-goal: never written. |

## Data-loss-shaped gaps

The write path is clean by construction — patches in the card's own dialect,
verbatim export, UID-keyed re-import, byte-identical undo on delete. Two
places still need an honest answer before the Milestone-2 exit can be
claimed:

1. **Concurrent remote edits before the conflict API exists.** The substrate's
   conflict surfacing (local/remote/base, per-property resolution) is a
   Milestone-1 item. Until it lands, verify what the sync engine does today
   when a card changed both locally and remotely between syncs — if either
   side's bytes can be overwritten without surfacing, that is a data-loss
   gap now, not a missing feature later.
2. **CSV re-import over an existing contact.** A mapped UID updates through
   the patcher; verify the semantics when a mapped column is empty for a row,
   or when the CSV's value for an unmapped-but-modeled field differs — an
   update must never clear fields the CSV does not carry.

Nothing else in the shipped surface loses data against the baseline; the
linking gap is a functionality gap, not a data-loss one.

## Ceiling gaps, ranked

By dependency order and by how much of the ceiling each unblocks:

1. **Linking** (03 §5) — the largest baseline gap *and* the prerequisite for
   the whole CRM layer, which attaches to persons, not cards.
2. **Duplicate review** (03 §6) — rides on linking (default action is link);
   closes the last GNOME Contacts aggregation gap.
3. **Notes + manual interaction log** — the first CRM tier; needs only the
   local-only collection, which has landed in the substrate.
4. **Keep-in-touch cadence + overdue list** — blocked on reminders moving to
   the substrate; this is the trigger for that move per the roadmap.
5. **RELATED relationships** — small on its own once linking exists.
6. **Per-person attachments** (local blob dir) — independent of sync work.
7. **Birthday feed to Slate** — substrate BDAY synthesis (Milestone 1).
8. **Automatic last-contacted** — blocked on Envelope's hook (Milestone 4).
9. **Baseline polish trio** — favorites, QR-code sharing, and the
   Google/Exchange account question (verify first).
10. **The undecided Monica rows** — journal, gifts, debts, per-contact tasks,
    calls/conversations, life events. Each needs a have/gap/rejected answer
    recorded here; an unlisted feature is a hole.
11. **LDAP read-only** — last, org-user audience, behind the substrate's
    collection abstraction.
