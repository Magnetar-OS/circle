# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/); versions follow
[Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

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
- Groups in the sidebar, read from the cards' `CATEGORIES`.
- A pop-launcher plugin: `con <name>` searches contacts; the context menu
  copies an email or phone number or starts a mail without opening a window.
- Single-instance activation: a second launch, `--new-contact`, `--search=`,
  and `.vcf` paths all reach the already-running window.
- Copy buttons and selectable text on every value in the detail pane.
- Live reload when anything else — a sync run, `khard`, an editor — changes
  the address book on disk.
