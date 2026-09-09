# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- **Attachments.** A scan, a contract, a photo of a whiteboard — kept with a
  contact and opened from the detail pane. Files are content-addressed, so
  attaching the same scan to three people stores it once, and a blob is
  deleted only when no record still names it. Nothing is written to the card:
  vCard *can* carry an attachment inline, and doing that would push a
  multi-megabyte PDF through CardDAV onto a server and into every other
  client. 25 MB per file, and the message says both numbers when something is
  over it.

- **Related people.** A card that carries relationships — vCard 4.0 `RELATED`,
  or the `X-ABRELATEDNAMES` form Apple Contacts writes — now shows them, and
  each one is a link when it names somebody in your address book. UIDs,
  `mailto:` values and plain names all resolve; a name that matches two people
  resolves to neither, and anything unresolved is kept as text rather than
  hidden. Read-only: Circle displays and navigates them but does not edit
  them.

- **Notes and keeping in touch** — the CRM layer. Timestamped notes per
  person, a "log a contact" button, and an optional cadence (weekly through
  yearly); anyone past theirs appears in a **Keep in touch** smart list in the
  sidebar. None of it is written to a card: it lives in `.crm/` beside the
  address books, invisible to the vdir layer and never sent to a server, so a
  shared book stays what other people put in it. A linked person's notes are
  the union of their cards', so linking and unlinking lose nothing; deleting a
  contact takes their notes with them, and undo brings both back.

- A conventions audit ([CONVENTIONS-AUDIT.md](CONVENTIONS-AUDIT.md)) covering
  all fifteen COSMIC checklist items with the evidence for each, the three
  problems it found, and the four places Circle knowingly diverges.
- A screenshot in the AppStream metainfo, and a translator's guide
  ([TRANSLATING.md](TRANSLATING.md)).

- **Greek**, the second language — which is what makes the project ready for
  translation contributions at all. The desktop entry, the applications-menu
  entry, and the software-centre summary are translated with it, because the
  xdgen build step reads the same catalogue the interface does.

- **Linking.** The same person in two accounts can be shown as one entry:
  tick both rows and press Link, or let Find duplicates propose them. The
  detail pane composes every card underneath — a shared address appears once,
  each value says which book it came from, and the linked cards are listed
  with an Unlink beside each. Nothing is merged: the cards stay byte for byte
  as they were and go on syncing to their own servers, so unlinking loses
  nothing. Links live in `.links/` beside the address books, invisible to the
  vdir layer and never pushed to a server.
- **Share as a QR code** (Edit → Share). A phone camera reads it straight
  into its own address book — no network, no account, no cable. The payload
  is rebuilt from the fields a phone files rather than copied from the card,
  because a real card with a photo is far past what a QR code can hold; a
  linked person's numbers all travel.
- **Texting through a paired phone.** Where KDE Connect is running with a
  device in reach, phone numbers gain a text button that hands the message to
  the phone. The button is absent otherwise rather than silently doing
  nothing, and nothing else in Circle depends on KDE Connect being installed.
  Placing calls stays with the desktop's own `tel:` handler.
- **Duplicate review** (Edit → Find duplicates). Candidates come from a
  shared address, a shared number across spellings, or names that
  transliterate alike (Γιώργος ↔ Giorgos — shown as a guess, not evidence).
  Each pair is reviewed side by side with the reason spelled out; the answers
  are Link or Not the same, and a dismissal is remembered. There is
  deliberately no merge button.

- Selection mode: the Select button beside the search field (or Ctrl+click,
  Shift+click for a range, Ctrl+A for everything) ticks rows, and the bar
  under the list deletes, exports, or adds the set to a group in one go.
- Deleting a single contact no longer interrupts with a dialog — it happens
  immediately, with an Undo toast that restores the card byte for byte.
  Deleting several at once still confirms first.
- An adaptive layout: below 640 px the panes collapse to one — list, or
  detail with a back button — and the window now shrinks to the 360 px the
  metainfo always claimed.
- Keyboard travel: ↑/↓ move through the list, Enter in the search field jumps
  to the first match, and Select all is Ctrl+A.

- CardDAV accounts and sync, in the app. The Accounts page (View → Accounts)
  lists the suite's shared accounts, adds one from a server address, a
  username, and a password, and syncs on demand; File → Sync now runs a pass
  too, and an optional background cadence (off by default) lives in settings.
  Every local edit, delete, import, and group change is queued for upload to
  the server its book is bound to.
- Generated avatars: a contact without a photo shows their initials on a
  colour seeded from their name — stable across launches, consistent between
  the list and the detail pane — so no row renders as an empty hole. The list
  now shows photos too, decoded once and cached, never per frame.
- A chosen photo is center-cropped square and scaled down to 512 px before it
  is embedded, so a camera photo does not become a 10 MB vCard. A photo that
  already fits, and anything undecodable, is stored byte-for-byte as chosen.

- Contact editing over the substrate's byte-preserving vCard patcher: names,
  emails, phones, addresses, organisation, job title, birthday, nicknames,
  websites, categories, and notes, with `TYPE` labels and one preferred entry
  per list. Photos, custom Apple labels, and every unmodelled property survive
  an edit.
- Creating and deleting contacts, with confirmation before a delete.
- Setting and removing a contact's photo, written in the card's own vCard
  dialect; inline photos display in the detail pane.
- New contacts are written as vCard 3.0 for server compatibility, with a
  settings toggle for 4.0. Existing contacts always keep their own version.
- `.vcf` import and export through the file portal. Import is keyed on UID, so
  re-importing an export updates instead of duplicating; opening a `.vcf` from
  a file manager imports it.
- Groups in the sidebar, in both the ways vCards spell them: `CATEGORIES`
  values, and `KIND:group` cards including Apple's
  `X-ADDRESSBOOKSERVER-KIND` form. Group cards can be created and deleted,
  and the editor toggles membership per group; membership edits rewrite only
  the group cards that changed, each in its own dialect.
- CSV import with an explicit column-mapping screen — samples shown per
  column, conservative pre-selection, nothing imported unconfirmed; a mapped
  Unique ID column makes re-imports update rather than duplicate.
- A pop-launcher plugin: `con <name>` searches contacts; the context menu
  copies an email or phone number or starts a mail without opening a window.
- Single-instance activation: a second launch, `--new-contact`, `--search=`,
  and `.vcf` paths all reach the already-running window.
- Copy buttons and selectable text on every value in the detail pane.
- Live reload when anything else — a sync run, `khard`, an editor — changes
  the address book on disk.

### Fixed

- The QR code's size limit was taken from the specification rather than from
  the encoder, and the two disagree — 2 953 bytes against the 2 331 the
  encoder actually accepts. A contact between the two passed the check and
  then failed to encode. The limit is measured now, by a test that finds it by
  bisection so it cannot drift.

- The QR code wrote a display heading into the card's `ORG` field, so a phone
  scanning a contact with a job title filed them under a company literally
  named "Mathematician, Acme". `ORG` now carries the company and its
  department levels as separate components and `TITLE` is its own property,
  and the payload is tested by parsing it back rather than by checking the
  text it was built from.

- A birthday with no year — `BDAY:--0415`, legal vCard and common from people
  who would rather not state an age — was not shown at all. It is now, as a
  day and a month, without inventing a year. **A related bug in the substrate
  is still open:** editing any field deletes such a birthday from the card
  entirely. Reported, with a failing acceptance test kept visible in Circle's
  suite.

- Editing any field stripped parameters from a contact's other lines —
  `X-SERVICE`, `PID`, `ALTID`, `LANGUAGE`, and a quoted `GEO=` on an address
  all vanished from a card whose *name* was edited, and the loss pushed to the
  server on the next sync. Fixed in the substrate; Circle now also pins that
  its editor mutates the entries it was given rather than rebuilding them,
  which is the half of that fix an application has to keep on its own.
- An organisation's department levels were dropped: `ORG:Company;Research;Team`
  became `ORG:Company`. They are preserved now, and shown.
- A category containing a comma was split in two by the editor, which joined
  categories with commas for display and split on every comma when saving.
  Commas inside a category are now written `\,`, the way vCard writes them.

- **A contact could be given another person's identity.** A `.vcf` holding
  several cards — which is what every export from Google, Apple and Outlook is
  — was read as several contacts that all carried the whole file as their
  source. Editing one of them then wrote that person's name, email and address
  over the *first* card in the file while leaving the first card's UID in
  place, and the edit never reached the person who made it. Fixed in the
  substrate; Circle now has a multi-card fixture and asserts, for every write
  it can perform, that the card it did not edit is byte-for-byte unchanged.
- Deleting one contact out of a shared file deleted everybody else in it.
- Two contacts whose ids differed only in a non-ASCII character shared one
  notes file, so one person's history appeared on another's card — the CRM
  store's file-name escape truncated each character to a single byte, which
  makes `α` and `±` the same name. Notes on a contact with a non-ASCII id were
  also unreadable after a restart, for the same reason in reverse. Both
  predate any release.
- **Re-importing an export deleted most of it.** Import matches a card to an
  existing contact by UID and writes to that contact's file — so importing a
  two-card export over itself wrote each card over the whole file in turn and
  left one contact. The card is now replaced inside the file, leaving its
  neighbours untouched. This is the promise "re-importing the same export
  updates rather than duplicates" doing the opposite of what it said.
- Setting a photo, or a group's membership, on one contact in a shared file
  wrote it onto a different contact's card.
- Undoing a delete overwrote the whole file, silently reverting any edit made
  to the other people in it while the undo toast was up. The undo entry now
  carries only the deleted card, and puts it back into whatever the file holds
  by then.

- Circle was shipping Slate's calendar icon — byte for byte the same file, so
  a contacts application showed a calendar in the applications menu, on the
  panel, in its own About page, and in the software centre. Replaced with a
  contact card, drawn once per size (16 through 64, plus scalable) on each
  size's own pixel grid.
- Icon lookups had no fallback chain, so a theme missing one would render a
  blank button with no warning. Every lookup now names its own replacements.
- CI built on whatever `stable` was rather than the 1.98 the project declares:
  the workflow passed a toolchain to an action, which overrides
  `rust-toolchain.toml`.
- The Rust version the manifest offered as supported was one nothing ever
  built. `rust-toolchain.toml` pinned the channel `1.98`, which floats to the
  newest 1.98.x, while `Cargo.toml` declared a 1.98.0 minimum — so every
  build, local and CI, ran 1.98.1 and 1.98.0 was never exercised. Nothing
  reported it, and nothing could: `rust-version` is a *minimum*, so a manifest
  asking for more than the pinned channel is refused outright while one asking
  for less builds in silence. Both are pinned to 1.98.1 now, and CI fails when
  they disagree.
- `circle --search=` now selects the person when the query names exactly one,
  which is what the launcher plugin's Enter always claimed to do. Two or more
  matches stay a list.

- The English catalogue defined twenty message ids twice, so half of the
  Accounts page's strings were silently shadowed by an older set. Deduplicated,
  and `tests/catalogues.rs` now fails the build on a repeated id, a missing
  translation, or one the application no longer uses.
- Every icon-only button — the mail, call, and browser actions, the remove
  buttons in the editor, and the back button in the narrow layout — now has a
  tooltip. An icon alone is not a name.
