# COSMIC conventions audit

A pass over the fifteen-item checklist in
[cosmic-conventions.md](cosmic-conventions.md), item by item, against the tree
as it stands. Each row says what was checked and how — a claim that cannot be
re-run is not evidence.

Three items were **not** already satisfied when this was written. All three are
fixed; they are described under [What this found](#what-this-found) rather than
buried in the table, because a clean table with no history is the least useful
kind of audit.

Re-run this audit when the conventions document changes or before a release.

## The checklist

| # | Item | State | How it was checked |
|---|---|---|---|
| 1 | Generated from a template, skeleton kept | follows | `src/` is the template shape: `main.rs` → `lib.rs::run`, `app.rs` with the `Application` impl, `config.rs`, `i18n.rs`. `update` is one match, the view split into `src/ui/`. |
| 2 | libcosmic unpinned, `Cargo.lock` committed, no separate `cosmic-config`, one comment per feature | follows | No `rev =` in `Cargo.toml`; `Cargo.lock` is tracked; `cosmic-config` is used through `cosmic::cosmic_config`; the `[dependencies.libcosmic]` block comments each of the six features. |
| 3 | `rust-toolchain.toml` and `rust-version` agree; CI takes the toolchain from the file | **fixed** | Both name 1.98. CI was overriding the file — see finding 1. |
| 4 | One RDNN id for config store, desktop entry, metainfo, icon, `StartupWMClass`, D-Bus name | follows | `io.github.entro314labs.Circle` in `app.rs`, `launcher.rs`, `justfile`, `resources/app.desktop` (`Icon=` and `StartupWMClass=`), and `resources/app.metainfo.xml` (`<id>` and `<launchable>`). |
| 5 | justfile with `rootdir` / `prefix` / `cargo-target-dir`; install wires nothing up; `just vendor` works; metainfo to `share/metainfo` | follows | All three variables present; `install` guards its two `update-*` cache calls on an empty `rootdir`; `vendor` and `build-vendored` present; `metainfo-dst` is `share/metainfo`, not the template's legacy `share/appdata`. Verified by staging: `just rootdir=… install` produces a complete tree. |
| 6 | `i18n/`, `i18n.toml`, `src/i18n.rs`; no user-visible literal left in code; plurals through Fluent | follows | Grep for literal strings in `text::*` and `button::*` constructors returns nothing. `fl!` is compile-time checked against `i18n/en`, so a missing id is a build error. Five ids use Fluent selectors for plurals. `tests/catalogues.rs` fails on a duplicate, missing, or stale id. |
| 7 | Config struct versioned; `watch_config` wired into `subscription` | follows | `Config` derives `CosmicConfigEntry` with `#[version = 1]`; `subscription` starts with `watch_config::<Config>(Self::APP_ID)`. |
| 8 | Shortcuts in a `KeyBind` table surfaced through a menu bar; trait hooks rather than listeners | follows | `key_bind::key_binds()` is the single table; `ui::menus` passes it to `menu::items`, which prints each accelerator. `on_search`, `on_escape`, `on_nav_select`, `on_window_resize` and `dbus_activation` are implemented; nothing re-listens for Ctrl+F or Escape. `on_app_exit` is not implemented — see [Considered and not done](#considered-and-not-done). |
| 9 | Theme spacing and radii; no raw pixels, no literal colours | follows, with two documented exceptions | Every padding, gap and radius reads from `cosmic::theme::spacing()` or `corner_radii`. The exceptions are the avatar palette and three pane widths — see [Considered and not done](#considered-and-not-done). |
| 10 | Every icon name resolves through COSMIC → Pop → hicolor; explicit fallback chain; own icon embedded for About | **fixed** | All 16 names resolve on this install (`find /usr/share/icons/{Cosmic,Pop,hicolor}`). Chains were missing, and the icon itself was wrong — see findings 2 and 3. About embeds the icon with `include_bytes!` + `from_svg_bytes`, so it renders from a `cargo run` that was never installed. |
| 11 | Compositor blur gated on the matching `frosted_*` key | n/a | Circle requests no blur. |
| 12 | Layer surfaces: `Layer::Overlay`, `exclusive_zone: -1`, create/destroy per appearance, `AppType::System` | n/a | Circle is an ordinary windowed application and creates no layer surfaces. |
| 13 | Launching applications: activation token, switcheroo GPU environment, `spawn_desktop_exec` with the systemd scope | partial, deliberate | The launcher plugin calls `spawn_desktop_exec` with `desktop-systemd-scope`, so the window belongs to the session rather than to pop-launcher. The other two do not apply — see [Considered and not done](#considered-and-not-done). |
| 14 | Metainfo carries `com.system76.CosmicApplication`, `requires`, `supports`, `branding`, a release matching Cargo.toml, reachable URLs; `MimeType=` on one line | follows | All present. `<release version="0.1.0">` matches `version = "0.1.0"`. `MimeType=` is a single 48-character line — the wrapped-list trap that silently registers nothing is not present here. |
| 15 | `desktop-file-validate` and `appstreamcli validate` in CI, `--no-net` on pull requests | follows | CI's last step runs `just validate-metadata`, which runs both against the **generated** files in `target/xdgen/`; `--no-net` is the default recipe and `validate-metadata-urls` is the networked one. |

## What this found

### 1. CI was building on a different toolchain than the project declares

`rust-toolchain.toml` pinned 1.98 and `Cargo.toml` agreed, but the workflow
used `dtolnay/rust-toolchain@stable`, and passing a toolchain to an action
overrides the file. CI was therefore green on whatever `stable` happened to be
that week, which is exactly the drift the convention exists to prevent — and
it fails open, so nobody notices until a 1.98-only or 1.98-incompatible
construct lands.

Fixed by removing the action. Rustup is preinstalled on the runner and installs
the channel and components named in `rust-toolchain.toml` on the first cargo
invocation. Verified locally: `rustup show active-toolchain` reports 1.98
"overridden by `rust-toolchain.toml`".

### 2. The icon was Slate's

`resources/icons/hicolor/scalable/apps/icon.svg` was **byte-identical** to
Slate's (`md5 24d4797c…`): a page with binder rings and a grid of days, with a
"today" marker. A contacts application was showing a calendar in the
applications menu, on the panel, in its own About page, and — through the
metainfo's remote icon URL — in the software centre.

Item 4 makes the icon part of the application's identity, so this is an
identity bug rather than a cosmetic one. Replaced with a contact card that
keeps the family resemblance (same rounded body, same paper gradient) and
differs in the two things the eye catches first: the header is Circle's own
branding purple (`#842bd2`, the AppStream `<branding>` primary) rather than
Slate's blue, and the shape on it is a person.

Drawn once per size — 16, 24, 32, 48, 64 and scalable — with coordinates on
each size's own pixel grid, which is item 10's higher-effort option. Below 32
the name lines are dropped: they are thinner than a pixel there and only muddy
the card. The justfile and the Flatpak manifest install the set; `debian/` gets
it for free because its `rules` calls the justfile.

### 3. Icon lookups had no fallback chain

Every lookup was a bare `icon::from_name(name)`, so libcosmic's default
fallback applied: truncate at each `-` and retry. That is harmless for
`mail-send-symbolic` (which degrades to `mail`) and useless for
`object-select-symbolic`, which degrades to `object` — a name no theme in the
chain ships. A lookup that finds nothing renders a **blank** icon and logs
nothing, so on a COSMIC install with a different icon theme a button would
simply be empty.

All 16 names resolve on the machine this was audited on, which is precisely why
this needed a test rather than an inspection. Every lookup now goes through
`ui::icon`, which names a per-icon chain toward the freedesktop standard names,
and `ui::tests::every_icon_used_in_the_source_has_a_fallback_chain` reads the
source to fail on any lookup added without one.

## Considered and not done

A deviation with a reason is worth more than a follow without one. These are
the places Circle knowingly does not follow the letter of the checklist.

- **`on_app_exit` (item 8) is not implemented.** The convention's point is not
  to re-listen for events the trait already offers, and Circle does not: it
  implements `on_search`, `on_escape` and the rest. There is nothing to flush
  at exit — every write reaches disk when it is made — so an empty hook would
  be ceremony. The one thing lost on exit is unsaved text in an open editor;
  a confirm-on-quit flow is a feature, not a convention item, and is not built.

- **A fixed colour palette in `ui::avatar` (item 9).** Generated avatars are
  seeded from a fixed eight-colour table rather than the theme. An identity
  colour that shifted when the desktop switched between light and dark would
  defeat the point of having one: the colour is there so the eye can find a row
  again without reading it. Every entry is dark enough for white text in both
  modes.

- **Three `Length::Fixed` pane widths (item 9).** The list pane is 320 and the
  account form's inputs are 220. Item 9 governs spacing and corner radii, which
  are theme concerns; a pane width is a layout decision with no token to read
  from. Slate spells its account form the same way, and the two applications
  showing the same form at different widths would be worse.

- **No activation token or switcheroo GPU environment when launching (item
  13).** The token is bound to the launching surface, and a pop-launcher plugin
  owns no surface to bind one to. The GPU preference applies to launching an
  arbitrary desktop entry that declares one; the plugin launches Circle itself.
  The third part of item 13 — `spawn_desktop_exec` with `desktop-systemd-scope`
  — is followed, and it is the part that matters here: without it the window
  would die with pop-launcher.

## Not covered by the checklist, and still open

- **Per-server CardDAV quirks** live in the substrate, not here.
- **A second screenshot.** The metainfo carries one. Capturing more needs a
  desktop where nothing else raises a window over Circle mid-capture.
