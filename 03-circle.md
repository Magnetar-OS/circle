# Circle — Contacts

## As built

Data layer complete and tested; shell, write path, photos (display),
import/export, and the launcher plugin all working. The substrate patcher
landed (`core::patch` + `vcard::patch_vcard`), so edits patch the stored card
— `item1.X-ABLabel`, PHOTO, IMPP, GEO survive by construction, pinned end to
end by `tests/write_path.rs`. Grouped entries are editable in value, shown
with their label, and deliberately not removable (dropping one from the model
would not remove it from the card). Single-instance over D-Bus:
`--new-contact`, `--search=`, and `.vcf` paths all hand over to the running
window, which is what backs the desktop entry's action and the MimeType
registration. The no-index decision (read whole, sort, filter; a few hundred
KB) is honest and removes a cache-invalidation surface; keep it until
profiling says otherwise.

## Gaps, dependency-ordered

### 1. Write path — DONE except the version choice

Done: field editors over patches, add/remove with TYPE labels, one PREF per
list, create, delete with confirmation, and the "other fields" honesty section
(detail pane and editor both list what the card carries that Circle won't
touch, in the card's own spelling).

Open: new cards serialise as vCard 4.0 only — `core::to_vcard` has no 3.0
writer, so the "3.0 default for Nextcloud compatibility" decision is blocked
in the substrate, not here. A settings toggle before the writer exists would
be a lie; add the writer first.

### 2. Photos — display DONE, write open

Done: `core::vcard::photo()` decodes both inline forms (3.0 `ENCODING=b`,
4.0 `data:`); the detail pane shows it, decoded once per selection, not per
frame. URI-form photos are surfaced as URIs and never fetched.

Open: set/replace/remove through the patcher (an `Edit::set` on PHOTO plus a
file chooser and base64 encode). Network avatar fetching stays opt-in only,
off by default, if ever.

### 3. Parity with Slate's shell — largely DONE

Done:
- **circle-launcher** ships: `con <query>`, Enter opens Circle filtered to
  the person (`--search=` through the single-instance hand-over), context
  menu copies email/phone (via `wl-copy` — the plugin owns no surface, so no
  toolkit clipboard) or composes via `xdg-open mailto:`.
- Import/export via portal, UID-keyed re-import, export per-book or all
  visible books, verbatim bytes both ways (`ContactStore::import_vcf` /
  `export_book`, multi-card files split into per-card verbatim segments).

Open:
- **tel:/sms: handoff** to KDE Connect where its D-Bus is present. The detail
  pane already emits `tel:`/`mailto:` through the desktop handler.
- CSV import with an explicit column-mapping screen, no silent guessing.
- Export in 3.0 — same substrate blocker as tier 1.

### 4. Groups

Both mechanisms, because servers disagree and this is a data-loss site:
CATEGORIES on cards *and* KIND:group / addressbook-group cards. Per-server
behavior goes in the substrate quirks table (01). UI: sidebar groups, drag or
menu assignment, group as compose-list.

### 5. Linking (the distinguishing model)

- App-level `person` = set of links to underlying cards (per account/book)
  with per-field display precedence; composed detail view with subtle
  per-field source indicators; each source card stays intact and syncs
  unchanged to its own server.
- Every edit writes to exactly one underlying card (the field's source, or a
  chosen default card for new fields).
- Link store: local-only collection (01) — a non-synced vdir directory of
  link records, consistent with files-as-truth.
- Destructive merge exists as a separate, explicit, undoable operation.

### 6. Duplicate review

- Candidates: exact email match; phone match after E.164 normalization
  (`phonenumber` crate — the search normalization already halfway there);
  fuzzy names *as suggestions only*.
- Name matching handles transliteration (Γιώργος ↔ Giorgos ↔ George):
  normalize (any_ascii/ICU) + nickname table before Jaro-Winkler/trigram.
- Review screen: side-by-side field diff; default action is **link** (5),
  not merge; no automatic merging, ever.

### 7. CRM layer (the gap between GNOME Contacts and Monica)

- Timestamped notes per person in the local-only collection — synced address
  books others see stay unpolluted; consistent with vdir (plain files,
  greppable).
- Last-contacted: manual "log interaction" now; automatic once Envelope can
  report mail sent/received for a linked address (library-level hook, no
  daemon needed — 00).
- Keep-in-touch cadence per person + "overdue" smart list; notifications
  reuse Slate's reminder machinery once it moves to the substrate (00) —
  do not write a second scheduler.
- Birthday feed to Slate via substrate BDAY synthesis (01).
- Relationships: RELATED (vCard4) as clickable links between persons,
  text fallback when the target is a 3.0 card.
- Attachments per person (card scan, PDF) in a local content-addressed blob
  dir; explicitly local-only in the UI. Business-card OCR is a heavyweight
  optional feature flag, last.

### 8. LDAP (optional, org users)

Read-only source via substrate (01): live search, cached, linkable into
persons, never written.

## Non-goals (unchanged)

Social scraping, third-party enrichment, auto-merge, attachment sync,
LDAP write-back.

## Risks

- vCard 3.0↔4.0 asymmetries (TEL URIs, label grouping) — conversions explicit,
  covered by the round-trip corpus.
- Server group divergence will produce per-server bugs post-tier-4; quirks
  table discipline.
- List performance is fine at "few thousand"; the linking view multiplies
  reads per row — re-profile at tier 5, add windowed rendering only if
  measurement demands it.
