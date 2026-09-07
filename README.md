# Circle

Contacts for the [COSMIC desktop](https://github.com/pop-os/cosmic-epoch), built
with [libcosmic](https://github.com/pop-os/libcosmic).

Contacts are plain vCard files in a
[vdir](https://vdirsyncer.pimutils.org/en/stable/vdir.html) layout, so they stay
readable by `khard`, `vdirsyncer`, and anything else that speaks `.vcf`.

## Part of a suite

Circle is one of three applications over a shared substrate,
[cosmic-pim](https://github.com/entro314-labs/cosmic-pim):

| App | Repository | What it is |
|---|---|---|
| **Slate** | [slate](https://github.com/entro314-labs/slate) | Calendar and tasks |
| **Circle** | you are here | Contacts |
| **Envelope** | [envelope](https://github.com/entro314-labs/envelope) | Mail (scaffold) |

The substrate owns the contact model, vCard parsing, the vdir on disk, CardDAV,
and accounts. This repository is the COSMIC front end over
`cosmic_pim_core::store::contacts::ContactStore` and very little else.

**Accounts are shared.** An account added in Slate is already here — one entry at
`$XDG_CONFIG_HOME/cosmic-pim/accounts.toml` with the password in the OS keychain,
not one per app.

[cosmic-pim/ARCHITECTURE.md](https://github.com/entro314-labs/cosmic-pim/blob/main/ARCHITECTURE.md)
is the canonical description of how the layers fit.

## State

Reading, searching, creating, editing, and deleting contacts all work.

- **Browsing** — an address-book sidebar, the contact list, and a detail pane.
  Every value in the detail pane is selectable and has a copy button, because
  taking a number out of an address book is the main thing anybody does with
  one.
- **Search** across names, emails, organisations, nicknames, categories, and
  phone numbers, punctuation-insensitive — `5551234` matches `+1 (555) 123-4`.
- **Editing** — names, emails, phones, addresses, organisation, job title,
  birthday, nicknames, websites, categories, and notes, with `TYPE` labels and
  one preferred (`PREF`) entry per list. Edits patch the stored card rather than
  rebuilding it; see [Editing](#editing).
- **Creating and deleting.** A single delete happens immediately with an Undo
  toast — the card comes back byte for byte; deleting several at once asks
  first.
- **Selection mode** — tick rows (Select button, Ctrl+click, Shift+click for
  a range, Ctrl+A for all) and delete, export, or add the set to a group in
  one action.
- **An adaptive layout.** Three panes on a wide window; below 640 px the list
  and detail take turns, down to 360 px.
- **Linking.** One person, several cards: the list shows one row, the detail
  pane composes the values from every card and says which book each came
  from, and Unlink takes one back out. Linking never rewrites a card — see
  [Linking](#linking).
- **Duplicate review** — candidates from a shared address, a shared number, or
  a transliterated name; reviewed in pairs, answered Link or Not the same,
  never merged automatically.
- **Photos and avatars.** Inline `PHOTO` data (both the 3.0 `ENCODING=b` and
  the 4.0 `data:` forms) is decoded and shown in the list and beside the name;
  a contact without a photo gets generated initials on a colour seeded from
  their name. The editor can set, replace, or remove the photo — a chosen
  image is center-cropped square and scaled to 512 px, then patched into the
  card in its own dialect. Remote photo URIs are deliberately never fetched —
  no network for avatars, by design.
- **Accounts and sync.** The Accounts page lists the suite's shared CardDAV
  accounts, adds one from a URL, username, and password, and syncs on demand
  or on a background cadence (off by default). Local edits, deletes, imports,
  and group changes are queued for upload to the server their book is bound
  to; a book with no binding stays local and queues nothing.
- **Version policy.** New contacts are written as vCard 3.0 — what Nextcloud
  and most CardDAV servers speak natively — with a settings toggle for 4.0.
  Existing contacts always keep the version their own bytes declare: edits
  patch in the card's dialect and never convert.
- **Groups**, both ways vCards spell them. `CATEGORIES` values and
  `KIND:group` cards (including Apple's `X-ADDRESSBOOKSERVER-KIND` form)
  share one sidebar section; pick one to filter the list. Group cards can be
  created and deleted from the File menu, and the editor toggles a contact's
  membership per group. Membership edits rewrite only the group cards that
  changed, in each card's own dialect — so a group written by an Apple
  client keeps the spelling Apple clients read.
- **Import and export**, as `.vcf` through the file portal. Import is UID-keyed
  so re-importing the same export updates rather than duplicates; export writes
  the stored bytes verbatim, so nothing is lost in either direction. Opening a
  `.vcf` from a file manager imports it too — the `MimeType=` registration is
  real.
- **A launcher plugin.** `con ada` in the COSMIC launcher finds Ada; Enter
  opens her in Circle, and the context menu copies her email or phone, or
  starts a mail, without opening a window at all.
- **Single instance.** A second `circle`, `circle --new-contact`, or
  `circle file.vcf` hands its request to the running window over D-Bus instead
  of opening a duplicate.
- **External changes are noticed.** A `vdirsyncer` run, `khard`, or a text
  editor changing a card updates the list without a restart.
- **Settings** persist through `cosmic-config`: which books are shown, which
  book new contacts go to, and whether to sort by first or last name.

- **CSV import** with an explicit column-mapping screen: every column is
  shown with a sample of its data and a target dropdown, obvious headers are
  pre-selected for confirmation, and nothing is imported until you say so.
  Map a column to Unique ID and re-importing the same file updates contacts
  instead of duplicating them.

- **Share as a QR code.** A phone camera reads the contact straight into its
  own address book. The code carries a rebuilt vCard 3.0 — name, numbers,
  addresses, organisation, websites — because a stored card with a photo is
  far past a QR code's capacity; the card on disk is untouched.
- **Texting through KDE Connect.** With the daemon running and a phone in
  reach, numbers gain a text button. Absent otherwise: a button that silently
  does nothing is worse than one that is not there. Calls stay with the
  desktop's `tel:` handler, which is the supported path.

- **Attachments and related people.** Files kept with a contact — scans,
  contracts — content-addressed so the same file attached twice is stored
  once, and local like the notes. Relationships a card already carries
  (vCard 4.0 `RELATED`, or Apple's `X-ABRELATEDNAMES`) are shown and are
  clickable when they name somebody in your address book.
- **Keeping in touch** — the layer between an address book and a CRM.
  Timestamped notes per person, a "log a contact" button, and an optional
  cadence (weekly through yearly) that puts somebody in the **Keep in touch**
  smart list when they fall past it. None of it touches a card: it lives in
  `.crm/` beside the books, so a shared address book stays what other people
  put in it — see [Notes and keeping in touch](#notes-and-keeping-in-touch).

Everything in [03-circle.md](03-circle.md)'s parity tiers is built, and the
CRM layer (§7) is built bar its two optional pieces: per-person attachments,
and `RELATED` relationship links. LDAP (§8) is untouched.

## Building

A [justfile](./justfile) is included for the [`just`](https://github.com/casey/just)
command runner.

```sh
just              # build-release
just run          # build and run
just install      # install binary, desktop entry, metainfo, and icon
just check-all    # formatting, clippy, tests, and metadata validation
```

Or with cargo directly:

```sh
cargo build --release
./target/release/circle
```

Requires a sibling checkout of `cosmic-pim` — see that repository's README for
why the dependency is a path rather than a git tag today.

Point it at sample data without touching your real address book:

```sh
just run-sandboxed          # writes to /tmp/circle-contacts
# or
COSMIC_PIM_CONTACTS_DIR=/tmp/contacts ./target/release/circle
```

The desktop entry and the AppStream metainfo are **generated** at build time by
[build.rs](build.rs) from the templates in [resources/](resources/) plus the
Fluent catalogues in [i18n/](i18n/), so the application's name and summary are
translated in the applications menu and the software centre and not only inside
the window. They land in `target/xdgen/`, which is what `just install` and
`just validate-metadata` use.

## Where things live

| Path | What |
| --- | --- |
| `~/.local/share/contacts/` | Your address books. One directory per book, one `.vcf` per contact. |
| `~/.config/cosmic-pim/accounts.toml` | Accounts, shared with Slate and Envelope. Passwords are in the keychain, never here. |
| `~/.config/cosmic/io.github.entro314labs.Circle/v1/` | Settings, via `cosmic-config`: hidden books, default book, sort order. |

A book looks like this, which is what `vdirsyncer` writes:

```
~/.local/share/contacts/
└── default/
    ├── displayname        "Contacts"
    ├── color              "#842bd2"
    └── 9f3ca1….vcf
```

There is deliberately **no index**. The calendar has one because a month view
needs range queries over expanded recurrences; an address book is read whole,
sorted, and filtered by substring. A few thousand contacts is a few hundred
kilobytes of text — faster to read than to invalidate a cache over.

## Editing

`Contact` models roughly a third of what a real vCard carries. The rest — PHOTO,
GEO, `X-ABLabel`, IMPP, anything a server or another client added — survives
because `store::contacts::write_contact` **patches** the source bytes rather
than re-serialising from the model. Change a name and the photo stays.

[tests/write_path.rs](tests/write_path.rs) pins this end to end: edit a synced
card through the real editor and the photo, the `GEO`, the `X-` properties, and
the Apple-grouped label are all still there afterwards.

Two things that patcher does shape what the editor offers:

- **Grouped entries are edited in place, never rewritten.** Apple attaches a
  custom label by grouping two lines (`item1.EMAIL` + `item1.X-ABLabel`).
  Rewriting the email as an ungrouped line would orphan the label — the classic
  vCard data-loss site. So in the editor a grouped entry has an editable value,
  its custom label shown as text, and **no remove button**: dropping it from the
  list would not remove it from the card, and a control that silently does
  nothing is worse than one that is absent.
- **An empty list means "remove", and only for entries this app owns.**
  Clearing the email list removes the ungrouped addresses and leaves grouped
  ones alone, because the model does not own their labels.

The detail pane and the editor both end with a short list of the properties the
card carries that Circle will not touch, so what is being preserved is visible
rather than merely promised.

## Linking

Two accounts holding the same person is the normal case, not an error, and
merging them is the one address-book operation that destroys what the files
can no longer reconstruct. So Circle links instead.

A *person* is a record naming two or more cards, stored as JSON in
`~/.local/share/contacts/.links/`. The dot makes it invisible to the vdir
collection scanner, so it is never listed as an address book and never
offered to a server as a collection — it is Circle's own metadata, beside
the books rather than inside them.

The cards themselves are not touched. Linking writes one small file; the
`.vcf` bytes are identical before and after, each card keeps syncing to its
own account, and unlinking restores two ordinary contacts with nothing lost.
[tests/linking.rs](tests/linking.rs) pins exactly that, byte for byte.

What the composed view does with two cards:

- **Lists union, duplicates collapse.** A shared address appears once, in the
  head card's spelling — showing it twice would make linking look like it
  made things worse. Numbers collapse across spellings, so `+30 210 1234567`
  and `2101234567` are one row.
- **The first card wins a scalar.** Organisation, birthday, note: the head's
  value if it has one, else the next card's. The head is the first card you
  picked when linking.
- **Every value says where it lives.** A linked person's rows carry their
  book's name, because that is which server an edit to that value would
  reach.

Editing a linked person edits **one** card — the head — and the editor says
which book that is. Values belonging to another card are edited by unlinking,
or by selecting that card in its own book.

## Notes and keeping in touch

An address book records who somebody is. What you last said to them, and how
often you meant to, is a different kind of fact — and it must not end up on a
card that syncs. A colleague sharing a work address book should not receive a
field saying when you last rang them.

So this data lives in `~/.local/share/contacts/.crm/`, one JSON file per card,
beside the books rather than inside them. The dot makes it invisible to the
vdir collection scanner, so it is never listed as an address book and never
offered to a server. [tests/crm.rs](tests/crm.rs) pins both halves of that:
that a note leaves the `.vcf` bytes untouched, and that `.crm/` never
registers as a collection.

Records are keyed by **card**, not by person. A linked person's notes are the
union of their cards' notes — the same rule the detail view uses for their
addresses — so linking and unlinking are lossless in both directions and
neither needs a migration. Deleting a contact takes their notes with them, and
undoing that delete brings both back.

Attachments follow the same rule and one more: their bytes live in
`.crm/blobs/`, named by the SHA-256 of their contents. Attaching one file to
three people stores it once, and a blob is removed only when no record names
it any more — the records *are* the reference count, so there is no counter
to drift. Deleting a contact does not sweep their blobs, because the delete is
undoable and bytes removed then could not come back; orphans are swept at
start-up instead, when nothing is mid-undo.

Relationships are the exception to all of this: they come off the **card**,
not from `.crm/`. Circle reads both spellings — vCard 4.0 `RELATED` and the
`X-ABRELATEDNAMES` form Apple Contacts writes — and makes each one a link when
its value names somebody in your address book. It does not edit them: writing
either back needs a byte-preserving patcher the substrate does not have, and
they stay listed under "also on this card" because that list is about what
Circle will not change.

There is no scheduler and there are no notifications. "Overdue" is a question
asked of the data when the list is drawn, not a timer: 03 §7 is explicit that
reminders wait for Slate's machinery to move into the substrate rather than
growing a second one here.

## Conventions

[CONVENTIONS-AUDIT.md](CONVENTIONS-AUDIT.md) walks the COSMIC conventions
checklist item by item with the evidence for each, including the three things
the audit found wrong and the four places Circle knowingly diverges.

## Translating

Two languages ship: English and Greek. Adding a third is one file —
[TRANSLATING.md](TRANSLATING.md) has the layout, the plural rule, and the
three ids that become the desktop entry rather than interface text.

## Licence

GPL-3.0-only for this application; the substrate it links is MPL-2.0. See
[LICENSING.md](LICENSING.md).
