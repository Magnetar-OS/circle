// SPDX-License-Identifier: GPL-3.0-only

//! Circle — contacts for the COSMIC desktop.
//!
//! The data layer is not here. Contacts, vCard parsing, the vdir on disk, and
//! CardDAV sync all live in `cosmic-pim`, shared with Slate (calendar) and
//! Envelope (mail). This crate is the COSMIC front end over
//! [`cosmic_pim_core::store::contacts::ContactStore`] and nothing more, which
//! is why it is small.

pub mod app;
pub mod config;
pub mod i18n;
pub mod key_bind;
pub mod ui;

/// Parses the command line into [`app::Flags`].
///
/// `--new-contact` backs the desktop entry's action; bare arguments are `.vcf`
/// paths, which is what backs the `MimeType=` registration — a file manager's
/// "Open With Circle" arrives here.
#[must_use]
pub fn parse_args(args: impl Iterator<Item = String>) -> app::Flags {
    let mut import = Vec::new();
    let mut new_contact = false;
    let mut search = None;

    for arg in args {
        if arg == "--new-contact" {
            new_contact = true;
        } else if let Some(query) = arg.strip_prefix("--search=") {
            search = Some(query.to_owned());
        } else if !arg.starts_with('-') {
            import.push(std::path::PathBuf::from(arg));
        } else {
            tracing::warn!(arg, "unknown argument ignored");
        }
    }
    app::Flags::new(import, new_contact, search)
}

/// Runs the application.
pub fn run() -> cosmic::iced::Result {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "circle=warn".into()),
        )
        .init();

    let requested_languages = i18n_embed::DesktopLanguageRequester::requested_languages();
    i18n::init(&requested_languages);

    let settings = cosmic::app::Settings::default()
        .size(cosmic::iced::Size::new(1100.0, 760.0))
        .size_limits(
            cosmic::iced::Limits::NONE
                .min_width(720.0)
                .min_height(480.0),
        );

    // `run_single_instance` rather than `run`: it is the only entry point that
    // sets `Core::single_instance`, which gates the D-Bus activation
    // subscription. Without it a second `circle file.vcf` opens a second
    // window instead of handing the file to the one already showing.
    cosmic::app::run_single_instance::<app::AppModel>(
        settings,
        parse_args(std::env::args().skip(1)),
    )
}

#[cfg(test)]
mod arg_tests {
    fn parse(args: &[&str]) -> crate::app::Flags {
        super::parse_args(args.iter().map(ToString::to_string))
    }

    #[test]
    fn vcf_paths_and_the_action_flag_are_recognised() {
        let flags = parse(&["--new-contact", "/tmp/a.vcf"]);
        assert!(flags.new_contact);
        assert_eq!(flags.import.len(), 1);
    }

    #[test]
    fn a_search_flag_is_carried_to_the_running_instance() {
        use cosmic::app::CosmicFlags;
        let flags = parse(&["--search=Ada Lovelace"]);
        assert_eq!(flags.search.as_deref(), Some("Ada Lovelace"));
        assert!(flags.action().is_some());
    }

    #[test]
    fn a_bare_launch_asks_for_nothing() {
        use cosmic::app::CosmicFlags;
        assert!(parse(&[]).action().is_none());
    }

    #[test]
    fn a_launch_with_work_carries_an_action_for_the_running_instance() {
        use cosmic::app::CosmicFlags;
        let flags = parse(&["--new-contact", "/tmp/a.vcf"]);
        assert!(flags.action().is_some());
        assert_eq!(flags.args(), vec!["/tmp/a.vcf"]);
    }
}
