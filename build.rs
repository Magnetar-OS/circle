// SPDX-License-Identifier: GPL-3.0-only

//! Generates the desktop entry and AppStream metainfo, with their user-visible
//! strings pulled from the same Fluent catalogue the application uses.
//!
//! The alternative — hand-maintained `.desktop` and `.metainfo.xml` files — has
//! the app's name and summary translated everywhere except the two places a
//! user meets them before the app is running: the applications menu and the
//! software centre. `xdgen` reads `i18n/<lang>/circle.ftl` and emits a
//! `Name[el]=` line (and an `<name xml:lang="el">` element) per language, so a
//! new translation reaches the launcher with no packaging change at all.

use std::{env, fs, path::Path};
use xdgen::{App, Context, FluentString};

fn main() {
    // Rebuilt when a translation or either template changes; without this the
    // generated files silently keep whatever the first build produced.
    println!("cargo:rerun-if-changed=i18n");
    println!("cargo:rerun-if-changed=resources/app.desktop");
    println!("cargo:rerun-if-changed=resources/app.metainfo.xml");

    let ctx = Context::new("i18n", env::var("CARGO_PKG_NAME").unwrap()).unwrap();
    let app = App::new(FluentString("app-title"))
        .comment(FluentString("app-comment"))
        .keywords(FluentString("app-keywords"));

    let desktop_entry = app.expand_desktop("resources/app.desktop", &ctx).unwrap();
    let metainfo = app
        .expand_metainfo("resources/app.metainfo.xml", &ctx)
        .unwrap();

    // Honour CARGO_TARGET_DIR — the template hardcodes `target/`, which breaks
    // the install recipes for anyone who redirects the target directory.
    let target = env::var("CARGO_TARGET_DIR").unwrap_or_else(|_| "target".to_owned());
    let output = Path::new(&target).join("xdgen");
    fs::create_dir_all(&output).unwrap();
    fs::write(output.join("app.desktop"), desktop_entry).unwrap();
    fs::write(output.join("app.metainfo.xml"), metainfo).unwrap();
}
