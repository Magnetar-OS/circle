// SPDX-License-Identifier: GPL-3.0-only

//! The Fluent catalogues, checked against each other.
//!
//! `fl!` is validated against `i18n/en` at compile time, so a missing English
//! string is a build error and needs no test. Everything *else* about the
//! catalogues is invisible until a user in that locale hits it:
//!
//! - A **duplicate key** silently shadows one of its definitions. This has
//!   happened here once already, when a second Accounts section was appended
//!   to a file that had one — the newer strings were simply never used.
//! - A **missing translation** falls back to English, which is correct
//!   behaviour and a poor way to discover that a catalogue has drifted.
//! - An **extra key** in a translation is dead weight from a string that was
//!   renamed or removed, and it is how a catalogue rots.
//!
//! Cheap to run, and it fails on the commit that causes it rather than on a
//! bug report months later.

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};

/// Every message id defined in a catalogue, and how many times each appears.
fn keys(path: &Path) -> HashMap<String, usize> {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|why| panic!("cannot read {}: {why}", path.display()));

    let mut found: HashMap<String, usize> = HashMap::new();
    for line in text.lines() {
        // A message starts at column zero with `id =`; anything indented is a
        // continuation, an attribute, or a selector arm.
        let Some((id, _)) = line.split_once(" =") else {
            continue;
        };
        if id.is_empty()
            || !id
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            continue;
        }
        *found.entry(id.to_owned()).or_default() += 1;
    }
    found
}

fn catalogue(locale: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("i18n")
        .join(locale)
        .join("circle.ftl")
}

/// Every locale directory shipped, `en` included.
fn locales() -> Vec<String> {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("i18n");
    let mut found: Vec<String> = std::fs::read_dir(&dir)
        .expect("i18n directory")
        .filter_map(Result::ok)
        .filter(|entry| entry.path().is_dir())
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    found.sort();
    found
}

#[test]
fn no_catalogue_defines_a_message_twice() {
    for locale in locales() {
        let repeated: Vec<String> = keys(&catalogue(&locale))
            .into_iter()
            .filter(|(_, count)| *count > 1)
            .map(|(id, count)| format!("{id} ×{count}"))
            .collect();
        assert!(
            repeated.is_empty(),
            "{locale} defines a message more than once, so one definition is \
             silently dead: {repeated:?}"
        );
    }
}

#[test]
fn every_translation_covers_the_english_catalogue() {
    let english: BTreeSet<String> = keys(&catalogue("en")).into_keys().collect();
    assert!(!english.is_empty(), "the English catalogue parsed as empty");

    for locale in locales().into_iter().filter(|l| l != "en") {
        let translated: BTreeSet<String> = keys(&catalogue(&locale)).into_keys().collect();

        let missing: Vec<&String> = english.difference(&translated).collect();
        assert!(
            missing.is_empty(),
            "{locale} is missing {} strings, which will silently fall back to \
             English: {missing:?}",
            missing.len()
        );

        let extra: Vec<&String> = translated.difference(&english).collect();
        assert!(
            extra.is_empty(),
            "{locale} translates strings the application no longer uses: {extra:?}"
        );
    }
}

/// The three ids `build.rs` feeds to xdgen for the desktop entry and the
/// AppStream metainfo. A locale missing one of these produces an untranslated
/// applications-menu entry, which is the one place nobody thinks to look.
#[test]
fn every_catalogue_carries_the_xdg_metadata_strings() {
    for locale in locales() {
        let found = keys(&catalogue(&locale));
        for id in ["app-title", "app-comment", "app-keywords"] {
            assert!(
                found.contains_key(id),
                "{locale} has no {id}, so its desktop entry and metainfo stay in English"
            );
        }
    }
}
