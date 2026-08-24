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
- **Creating and deleting**, with a confirmation dialog before a delete.
- **Photos display.** Inline `PHOTO` data (both the 3.0 `ENCODING=b` and the
  4.0 `data:` forms) is decoded and shown beside the name. Remote photo URIs
  are deliberately never fetched — no network for avatars, by design.
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

Not yet: setting or removing a photo, groups as first-class objects, and CSV
import — see [03-circle.md](03-circle.md) for the order those come in.

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

## Licence

GPL-3.0-only for this application; the substrate it links is MPL-2.0. See
[LICENSING.md](LICENSING.md).
