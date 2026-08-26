# Circle — Roadmap

The destination: a contacts application that stands 1:1 against the best
existing contact apps, is pixel-perfect on COSMIC, feels immediate and
reactive, and integrates completely with the desktop and the rest of the
suite. This document orders the work; [03-circle.md](03-circle.md) remains the
detailed gap analysis it draws from, and
[cosmic-conventions.md](cosmic-conventions.md) is the integration yardstick.

## The benchmark

**Baseline parity target: GNOME Contacts.** It is the reference contacts app
on the Linux desktop and the bar a COSMIC user will measure Circle against.
**Stretch targets: KAddressBook and macOS Contacts** for the features GNOME
Contacts deliberately lacks (groups, printing, LDAP, smart lists). The CRM
tier (03 §7) is the layer none of them have — the differentiator, not the
baseline.

Where Circle already stands against GNOME Contacts:

| Capability | GNOME Contacts | Circle today |
|---|---|---|
| Browse / search / detail | yes | yes — plus punctuation-insensitive phone search |
| Create / edit / delete | yes | yes — byte-preserving patches, which GNOME does not do |
| Photos | select, crop, generated initials | display, set/replace/remove; no crop, no generated avatars |
| Address books | local + online accounts, in-app | local vdir; accounts exist in `accounts.toml` but no in-app UI |
| Sync | EDS does it underneath | external only (`vdirsyncer`); `cosmic-pim-sync` not yet wired in |
| Import / export vCard | yes | yes — UID-keyed re-import, verbatim export |
| Linking duplicates | yes, with suggestions | no (03 §5–6) |
| Bulk selection (delete, export, link) | yes | no |
| Share as QR code | yes | no |
| Adaptive narrow layout | yes (single pane on phones) | no — three panes always |
| Groups | **no** | yes — `CATEGORIES` (Circle is ahead) |
| Birthday | yes | yes |

So the parity gap is concrete and finite: **accounts+sync UI, linking,
avatars, bulk selection, QR share, adaptive layout.** Everything else is
polish, integration, or beyond-parity.

---

## Track A — Feature parity

Dependency-ordered. Each item lists its acceptance test.

### A1. Accounts and sync, in-app

`cosmic-pim-accounts` and `cosmic-pim-sync` are declared in Cargo.toml and
currently unused — the wiring is the work, not the plumbing.

- Settings section listing accounts from `accounts.toml` (shared with Slate —
  render, don't own; adding/editing an account should be one implementation in
  the substrate/suite, not re-built here).
- Manual "Sync now" per account and on Ctrl+R; sync status surfaced (spinner
  in the sidebar row, toast on failure with the actual error, never silent).
- Background sync on an interval from settings, off by default.
- **Done when:** a Nextcloud account added in Slate syncs contacts from
  Circle's own UI, a pulled remote change appears without restart (the
  watcher already covers this), and a sync error is visible and readable.

### A2. Avatars complete

- Generated initials avatars (name-seeded colour from the theme palette) for
  contacts without a `PHOTO` — list, detail pane, and launcher plugin.
- Crop/scale step when setting a photo (square crop; the substrate patcher
  already owns the write).
- **Done when:** no contact renders as an empty circle anywhere, and setting
  a photo from a large photo produces a reasonably-sized, cropped `PHOTO`.

### A3. Bulk selection

- Selection mode on the list (Ctrl+click, Shift+click, Ctrl+A), with a
  contextual action bar: delete (one confirmation for N), export selected,
  assign to group, and — once A4 lands — link.
- **Done when:** selecting 30 contacts and exporting produces one `.vcf`
  with 30 verbatim cards.

### A4. Linking and duplicate review *(the distinguishing model — 03 §5–6)*

- App-level person = links over underlying cards, per-field precedence,
  composed detail view with source indicators; every edit writes to exactly
  one card. Link store as a local-only vdir collection.
- Duplicate candidates: exact email, E.164-normalised phone
  (`phonenumber` crate), fuzzy names with transliteration as suggestions
  only. Review screen defaults to **link**, never auto-merges; destructive
  merge is separate, explicit, undoable.
- **Done when:** the same person from two accounts shows once, edits sync
  back to the right server, and unlinking restores two intact cards.

### A5. Sharing and interchange

- Share contact as QR code (vCard payload, rendered in-app — no network).
- CSV import with an explicit column-mapping screen, no silent guessing.
- `tel:`/`sms:` handoff to KDE Connect where its D-Bus name is present;
  fall back to the desktop handler otherwise.
- **Done when:** a phone scans the QR into its own contacts, and a Google
  CSV export imports with the user having confirmed every mapped column.

### A6. Groups, second half

- `KIND:group` / addressbook-group cards — gated on the substrate's
  per-server quirks table, as planned; do not ship ahead of it.
- Drag a contact onto a sidebar group to assign; group → "compose to all"
  once Envelope can take a recipient list.

### A7. Beyond parity (stretch, post-1.0 unless pulled forward)

- CRM layer (03 §7): timestamped notes, keep-in-touch cadence + overdue
  smart list, last-contacted (manual now, Envelope-fed later), RELATED
  links, local attachments. All in the local-only collection — synced books
  stay unpolluted.
- Printing (labels/list) and LDAP read-only — org-user features, after the
  substrate offers them.

## Track B — Pixel-perfect, delightful, reactive

Runs alongside Track A; every A-item lands already meeting this bar.

- **Conventions audit against the checklist** (cosmic-conventions.md §Checklist):
  every spacing value a token, every colour a semantic accessor, every
  container/button a semantic class; icon names verified along
  COSMIC → Pop → hicolor with explicit fallback chains; own icon embedded
  for About. One pass over the existing UI, then enforced in review.
- **Adaptive layout.** Three panes on wide windows; collapse to
  list ⇄ detail with a back button below a width breakpoint, and further to
  a single pane. Circle should be usable at 360 px, which is also what the
  metainfo's `display_length` claims.
- **Keyboard-first.** Type-ahead in the list, Ctrl+F via `on_search` (never
  a second listener), Escape closes panes in order, arrow navigation
  list→detail, every action in the menu bar with its shortcut printed from
  the KeyBind table. Full flow — find, copy a number, close — with no mouse.
- **Reactive feedback.** Skeleton/empty states with a call to action (empty
  book → "New contact"); toasts for outcomes (`widget::toaster`); **undo for
  delete** — replace the confirmation dialog with a 5-second undo toast for
  single deletes (keep confirmation for bulk); animations via
  `cosmic::iced::animation` for pane transitions and selection, interruptible,
  never blocking.
- **Performance budget.** Cold start to painted list < 300 ms at 1 000
  contacts; search keystroke to filtered list < 16 ms; no per-frame decoding
  (photos already decode once per selection — keep that discipline). The
  no-index decision stands until profiling says otherwise; the linking view
  (A4) is the trigger to re-profile and, only if measured, add windowed
  list rendering.
- **Accessibility.** Every interactive element reachable by keyboard and
  labelled; respect the system font scale; verify contrast in both themes.

## Track C — 100 % COSMIC integration

The conventions doc is the spec; Circle already follows most of it (xdgen,
single-instance D-Bus, `watch_config`, KeyBind table, portal dialogs, wgpu,
launcher plugin). What remains:

- `rust-toolchain.toml` + `rustfmt.toml` (`imports_granularity = "Module"`),
  agreeing with `rust-version` — one formatting commit, then stable.
- Per-size icons under `hicolor/<size>/apps/` instead of a single scalable —
  the higher-effort option the first-party apps ship.
- Second language in `i18n/` and Weblate onboarding; the xdgen pipeline
  means the desktop entry and metainfo translate for free.
- Metainfo completeness: screenshots, `branding` colours, releases matching
  Cargo.toml — `just validate-metadata` already checks the rest.
- **Peripheral apps:** birthdays feed Slate via substrate BDAY synthesis
  (substrate item, tracked here); `mailto:` compose prefers Envelope when it
  can receive one; launcher plugin stays in lockstep with app features
  (groups and linked persons should be searchable from `con` too).
- **Packaging:** `debian/`, Flatpak manifest, `flake.nix`,
  `hooks/pre-commit.hook` — the artefacts that arrive "when packaging
  starts"; that time is milestone 1.0. `just vendor` must keep working.

## Track D — Architecture and code quality

- **Split `app.rs` before it hits ecosystem-typical size** (1 475 lines
  today; convention tolerates ~2 000 with `view.rs` split out). Extract
  `view` composition into `src/ui/`; keep `update` a single match, per
  convention.
- **Tests grow with each Track A item**: linking gets its own round-trip
  suite in the style of `write_path.rs` (real files, no mocks); sync UI
  tested against a local CardDAV fixture (Radicale in CI); duplicate
  detection gets a transliteration corpus (Γιώργος ↔ Giorgos ↔ George).
- CI keeps `check-all` green; add `cargo deny` (licences + advisories) and
  the networked `appstreamcli` pass on main.
- Housekeeping now: `cosmic-pim-sync`/`cosmic-pim-accounts` are unused
  dependencies until A1 lands — either wire them (A1) or drop them until
  then; an unused git-path dependency still costs every builder.

---

## Milestones

Each milestone is releasable and tagged; Track B/C/D work is folded into
whichever milestone touches that surface.

| Version | Theme | Contents |
|---|---|---|
| **0.2** | Sync in the open | A1 accounts+sync UI · A2 avatars · toolchain/rustfmt files · unused-deps resolved |
| **0.3** | Many at once | A3 bulk selection · adaptive layout · undo-toast delete · keyboard-first pass |
| **0.4** | One person, many cards | A4 linking + duplicate review · re-profile list, window it only if measured |
| **0.5** | In and out | A5 QR · CSV import · KDE Connect handoff · A6 groups (if quirks table ready) |
| **1.0** | Feature-complete, packaged | GNOME-Contacts parity closed · conventions audit signed off · a11y pass · screenshots+branding · debian/Flatpak/nix · Weblate live |
| **post-1.0** | The layer nobody else has | A7 CRM tier · printing · LDAP |

## Non-goals (unchanged from 03)

Social scraping, third-party enrichment, auto-merge, attachment sync, LDAP
write-back — and network avatar fetching stays off by default, if ever.

## Risks

- **Substrate coupling.** A1, A6, birthdays-to-Slate, and the quirks table
  are substrate work surfaced here. Circle milestones must not silently
  block on cosmic-pim; when they would, ship the milestone without the item
  and say so in the changelog.
- **libcosmic tracks a moving branch.** Unpinned by convention; a breaking
  toolkit change can land any week. `Cargo.lock` is the shield; budget for
  an update pass per milestone.
- **vCard asymmetries and server group divergence** — unchanged from 03;
  conversions stay explicit, the round-trip corpus grows with every new
  property touched.
