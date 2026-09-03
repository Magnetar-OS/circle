// SPDX-License-Identifier: GPL-3.0-only

//! A `pop-launcher` plugin, so contacts are searchable from the COSMIC
//! launcher: `con ada` finds Ada, Enter opens her in Circle, and the context
//! menu copies her email or phone without opening anything.
//!
//! The protocol is newline-delimited JSON on stdin and stdout. Rather than
//! depend on `pop-launcher` — which would pull its whole dependency tree in for
//! six message shapes — the handful of messages used here are built directly,
//! the same choice Slate's plugin makes.
//!
//! Requests arrive as `{"Search":"..."}`, `{"Activate":0}`, `{"Context":0}`,
//! `{"ActivateContext":{"id":0,"context":1}}`, or the bare strings `"Exit"` /
//! `"Interrupt"`. Responses are `{"Append":{...}}` followed by `"Finished"`,
//! `{"Context":{...}}` for the option list, and `"Close"` after acting.

use circle::config::Config;
use circle::fl;
use cosmic_pim_core::model::Contact;
use cosmic_pim_core::store::contacts::ContactStore;
use serde_json::{Value, json};
use std::io::{BufRead, Write};

const APP_ID: &str = "io.github.entro314labs.Circle";

/// Results returned for one query.
const MAX_RESULTS: usize = 12;

fn main() {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "circle=warn".into()),
        )
        .init();

    // Results are shown to a person, so they follow the desktop's language.
    circle::i18n::init(&i18n_embed::DesktopLanguageRequester::requested_languages());

    let mut plugin = Plugin::new();
    let stdin = std::io::stdin();

    for line in stdin.lock().lines() {
        let Ok(line) = line else { break };
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        let Ok(request) = serde_json::from_str::<Value>(line) else {
            tracing::warn!(line, "unparseable request");
            continue;
        };

        if !plugin.handle(&request) {
            break;
        }
    }
}

/// What one context-menu entry does when chosen.
enum ContextAction {
    CopyEmail(String),
    CopyPhone(String),
    Compose(String),
}

struct Plugin {
    store: Option<ContactStore>,
    config: Config,
    /// Results from the last search, indexed by the id sent to the launcher.
    results: Vec<Contact>,
    /// Context options for the row a `Context` request named, in the order
    /// they were sent.
    context: Vec<ContextAction>,
    /// Drives the one async call this plugin makes: detaching the process it
    /// spawns. Built once because the plugin outlives many queries.
    runtime: Option<tokio::runtime::Runtime>,
}

impl Plugin {
    fn new() -> Self {
        let store = match ContactStore::open_default() {
            Ok(store) => Some(store),
            Err(why) => {
                tracing::error!(%why, "launcher plugin cannot open the contact store");
                None
            }
        };

        let runtime = match tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
        {
            Ok(runtime) => Some(runtime),
            Err(why) => {
                tracing::error!(%why, "no async runtime; results will not open");
                None
            }
        };

        Self {
            store,
            config: load_config(),
            results: Vec::new(),
            context: Vec::new(),
            runtime,
        }
    }

    /// Handles one request. Returns `false` when the launcher wants us to exit.
    fn handle(&mut self, request: &Value) -> bool {
        // Unit variants arrive as bare strings.
        if let Some(name) = request.as_str() {
            match name {
                "Exit" => return false,
                // Interrupt cancels an in-flight search; ours are synchronous.
                "Interrupt" | "Quit" => return true,
                _ => return true,
            }
        }

        if let Some(query) = request.get("Search").and_then(Value::as_str) {
            self.search(query);
        } else if let Some(id) = request.get("Activate").and_then(Value::as_u64) {
            self.activate(id as usize);
        } else if let Some(id) = request.get("Context").and_then(Value::as_u64) {
            self.send_context(id as usize);
        } else if let Some(request) = request.get("ActivateContext") {
            let id = request.get("id").and_then(Value::as_u64).unwrap_or(0);
            let option = request.get("context").and_then(Value::as_u64).unwrap_or(0);
            self.activate_context(id as usize, option as usize);
        }

        true
    }

    fn search(&mut self, query: &str) {
        self.results.clear();

        // The launcher strips the plugin prefix, but be tolerant of both forms.
        let needle = query.trim().trim_start_matches("con ").trim().to_owned();

        if needle.is_empty() {
            send(&json!("Finished"));
            return;
        }

        // Settings may have changed since the plugin was started; it is
        // long-lived, and a book hidden in Circle should be hidden here too.
        self.config = load_config();

        let Some(store) = self.store.as_mut() else {
            send(&json!("Finished"));
            return;
        };

        // Pick up anything synced since the last query.
        store.refresh();

        let mut matches: Vec<Contact> = store
            .search(&needle)
            .into_iter()
            .filter(|c| !self.config.is_hidden(&c.addressbook_id))
            .collect();
        matches.truncate(MAX_RESULTS);

        for (id, contact) in matches.iter().enumerate() {
            send(&json!({
                "Append": {
                    "id": id,
                    "name": contact.label(),
                    "description": describe(contact),
                    "keywords": Value::Null,
                    "icon": { "Name": "avatar-default-symbolic" },
                    "exec": Value::Null,
                    "window": Value::Null,
                }
            }));
        }

        self.results = matches;
        send(&json!("Finished"));
    }

    /// Enter on a row: open the contact in Circle.
    fn activate(&mut self, id: usize) {
        if let Some(contact) = self.results.get(id) {
            // Searching for the label is the hand-over: Circle raises its
            // window (or starts) and the query narrows the list to this
            // person. shlex parses the exec line, so the label is
            // single-quoted the way a shell would want it. The canonical
            // launch path underneath — double fork + setsid + a systemd
            // transient scope — so the window is the session's, not this
            // plugin's, which pop-launcher owns and reaps. No activation
            // token: a plugin owns no surface to bind one to.
            let quoted = contact.label().replace('\'', "'\\''");
            let exec = format!("circle '--search={quoted}'");
            match self.runtime.as_ref() {
                Some(runtime) => runtime.block_on(cosmic::desktop::spawn_desktop_exec(
                    exec,
                    std::iter::empty::<(&str, &str)>(),
                    Some(APP_ID),
                    false,
                )),
                None => tracing::warn!("no runtime; cannot launch the app"),
            }
        }
        send(&json!("Close"));
    }

    /// Right arrow on a row: offer copy and compose without opening anything.
    fn send_context(&mut self, id: usize) {
        self.context.clear();

        let Some(contact) = self.results.get(id) else {
            send(&json!({ "Context": { "id": id, "options": [] } }));
            return;
        };

        let mut options = Vec::new();
        if let Some(email) = Contact::preferred(&contact.emails) {
            self.context
                .push(ContextAction::CopyEmail(email.value.clone()));
            options.push(json!({
                "id": self.context.len() - 1,
                "name": format!("{} — {}", fl!("copy-email"), email.value),
            }));
            self.context
                .push(ContextAction::Compose(email.value.clone()));
            options.push(json!({
                "id": self.context.len() - 1,
                "name": fl!("send-mail"),
            }));
        }
        if let Some(phone) = Contact::preferred(&contact.phones) {
            self.context
                .push(ContextAction::CopyPhone(phone.value.clone()));
            options.push(json!({
                "id": self.context.len() - 1,
                "name": format!("{} — {}", fl!("copy-phone"), phone.value),
            }));
        }

        send(&json!({ "Context": { "id": id, "options": options } }));
    }

    fn activate_context(&mut self, _id: usize, option: usize) {
        match self.context.get(option) {
            Some(ContextAction::CopyEmail(value) | ContextAction::CopyPhone(value)) => {
                copy_to_clipboard(value);
            }
            Some(ContextAction::Compose(email)) => {
                if let Some(runtime) = self.runtime.as_ref() {
                    // Through the desktop's handler, so it reaches whatever the
                    // user's mail app is — Envelope once it registers mailto:.
                    runtime.block_on(cosmic::desktop::spawn_desktop_exec(
                        format!("xdg-open mailto:{email}"),
                        std::iter::empty::<(&str, &str)>(),
                        None,
                        false,
                    ));
                }
            }
            None => {}
        }
        send(&json!("Close"));
    }
}

fn describe(contact: &Contact) -> String {
    let email = Contact::preferred(&contact.emails).map(|e| e.value.clone());
    let phone = Contact::preferred(&contact.phones).map(|p| p.value.clone());
    match (email, phone) {
        (Some(e), Some(p)) => format!("{e} · {p}"),
        (Some(one), None) | (None, Some(one)) => one,
        (None, None) => contact.organisation.clone().unwrap_or_default(),
    }
}

/// Copies through `wl-copy`, the way pop-launcher's own calculator plugin does:
/// this process owns no Wayland surface, so there is no toolkit clipboard to
/// use. Silently a no-op when wl-copy is missing — the option still closed the
/// launcher, and stderr carries the reason for anyone debugging.
fn copy_to_clipboard(value: &str) {
    use std::process::{Command, Stdio};

    let spawned = Command::new("wl-copy")
        .arg("--")
        .arg(value)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
    if let Err(why) = spawned {
        tracing::warn!(%why, "wl-copy failed; nothing was copied");
    }
}

fn send(value: &Value) {
    let mut stdout = std::io::stdout().lock();
    if serde_json::to_writer(&mut stdout, value).is_ok() {
        let _ = stdout.write_all(b"\n");
        let _ = stdout.flush();
    }
}

fn load_config() -> Config {
    use cosmic::cosmic_config::CosmicConfigEntry;

    cosmic::cosmic_config::Config::new(APP_ID, Config::VERSION)
        .ok()
        .map(|handler| match Config::get_entry(&handler) {
            Ok(config) => config,
            Err((_errors, config)) => config,
        })
        .unwrap_or_default()
}
