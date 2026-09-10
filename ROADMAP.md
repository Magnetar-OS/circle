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

Where Circle stands against GNOME Contacts. This table was the gap list the
plan below was built from; it is kept as written, with the column on the
right updated as each row closed. [PARITY.md](PARITY.md) is the fuller audit.

| Capability | GNOME Contacts | Circle |
|---|---|---|
| Browse / search / detail | yes | yes — plus punctuation-insensitive phone search |
| Create / edit / delete | yes | yes — byte-preserving patches, which GNOME does not do |
| Photos | select, crop, generated initials | yes — crop and downscale on set, generated initials everywhere |
| Address books | local + online accounts, in-app | yes — vdir plus the suite's shared accounts, in-app |
| Sync | EDS does it underneath | yes — CardDAV on demand or on a cadence, with write-back queueing |
| Import / export vCard | yes | yes — UID-keyed re-import, verbatim export |
| Linking duplicates | yes, with suggestions | yes — and unlinking restores both cards byte for byte |
| Bulk selection (delete, export, link) | yes | yes — plus add-to-group |
| Share as QR code | yes | yes |
| Adaptive narrow layout | yes (single pane on phones) | yes — collapses at 640 px, usable to 360 px |
| Groups | **no** | yes — `CATEGORIES` *and* `KIND:group` cards |
| Birthday | yes | yes |
| CSV import | **no** | yes — with an explicit column-mapping screen |
| Text a contact | **no** | yes, through KDE Connect where it is running |

Baseline parity is closed. What remains is beyond-parity work (the CRM tier,
printing, LDAP) and the release engineering listed under Milestones.

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

| Version | Theme | Contents | State |
|---|---|---|---|
| **0.2** | Sync in the open | A1 accounts+sync UI · A2 avatars · toolchain/rustfmt files · unused-deps resolved | **done** |
| **0.3** | Many at once | A3 bulk selection · adaptive layout · undo-toast delete · keyboard-first pass | **done** |
| **0.4** | One person, many cards | A4 linking + duplicate review · re-profile list, window it only if measured | **done** — profiling deferred, see below |
| **0.5** | In and out | A5 QR · CSV import · KDE Connect handoff · A6 groups (if quirks table ready) | **done** |
| **1.0** | Feature-complete, packaged | GNOME-Contacts parity closed · conventions audit signed off · a11y pass · screenshots+branding · debian/Flatpak/nix · Weblate live | **done** — one caveat below |
| **post-1.0** | The layer nobody else has | A7 CRM tier · printing · LDAP | CRM **done** bar OCR; printing and LDAP not started |

### What 1.0 closed

Everything in Track A is built and the parity table above is closed.

- **Conventions audit** — [CONVENTIONS-AUDIT.md](CONVENTIONS-AUDIT.md), all
  fifteen items with the evidence for each. It found three real problems (CI
  building on the wrong toolchain, the icon being Slate's, no icon fallback
  chains) and records four deliberate divergences.
- **Icons** — named for the application id, as item 4 requires:
  `com.magnetaros.Circle.svg` scalable, PNGs rasterised from it at eight sizes
  through 512, and a monochrome `-symbolic` variant for the panel and the app
  grid. This is where the audit found Circle had been shipping Slate's calendar
  icon byte for byte; a repeat of that now fails the build, because each source
  names itself and a test asserts the embedded bytes carry Circle's app id.
- **Screenshot** — one, in the metainfo, taken on a real COSMIC session
  against a scratch address book.
- **Translation** — Greek shipped earlier; [TRANSLATING.md](TRANSLATING.md)
  now documents the layout, the plural rule, and the three ids that become the
  desktop entry. Hosted Weblate needs a project pointed at `i18n/`; everything
  on this side of that is in place.
- **Accessibility** — every icon-only button has a tooltip, every action is
  reachable from the menu bar with its accelerator printed from the KeyBind
  table, and the layout collapses to one pane at 640 px.

**The caveat:** the metainfo carries **one** screenshot rather than the three
or four a software centre shows best. Capturing more needs a desktop where
nothing raises a window over Circle mid-capture; the attempts here kept
catching other applications and were discarded.

### The CRM tier, and what is left of it

Built: timestamped notes, logged interactions and last-contacted, a per-person
cadence, and the overdue smart list — all in `.crm/` beside the books, keyed
by card so linking unions them and unlinking returns them.

Also built: **attachments**, content-addressed under `.crm/blobs/` with the
records as the reference count and an orphan sweep at start-up; and
**relationships**, read from the card in both spellings (`RELATED` and
Apple's `X-ABRELATEDNAMES`) and clickable when they resolve.

Not built, and each for a stated reason:

- **Editing relationships.** Circle reads and follows them; writing either
  property back needs a byte-preserving patcher the substrate does not have,
  and it does not model contact `RELATED` at all. Reading needed none of
  that, and a card arriving with relationships is now navigable rather than
  invisible.
- **Business-card OCR.** 03 §7 puts this behind a feature flag, explicitly
  last. Attachments exist without it.
- **Reminders and notifications.** Deliberately absent: 03 §7 says to reuse
  Slate's machinery once it moves to the substrate rather than write a second
  scheduler. "Overdue" is therefore a question asked when the list is drawn.
- **Automatic last-contacted from Envelope.** Waits on Envelope reporting mail
  sent and received per address; the manual button is the half that can exist
  without it.
- **Birthday feed to Slate.** Substrate work (BDAY synthesis), not Circle's.

### Deferred deliberately

- **List profiling and windowed rendering.** The 0.4 exit criterion was to
  re-profile once linking multiplies reads per row and to add windowing
  *only if measurement demands it*. Composition happens once per selection,
  not per row — the list still renders one card per row — so the trigger has
  not fired. Measure against a few thousand contacts before changing this.
- **KDE Connect against a live daemon.** The URI and object-path
  construction are tested and the absent case is pinned, but nothing has run
  against a real KDE Connect: it is not installed on the development
  machine. Treat the send path as unverified until someone with a paired
  phone tries it.

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
