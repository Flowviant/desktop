//! START AT SIGN-IN IS A PERSON'S PRESS (2026-09-29).
//!
//! Through 0.0.15 the app registered itself to start at sign-in the first
//! time it ran, unasked (an `autostart-initialized` marker kept that to
//! once). A Run entry nobody asked for is a system change without a warning,
//! which signed software may not make (SignPath Foundation's terms). So the
//! entry is written only by a press: the window's switch, "Start Flowviant
//! when I sign in to Windows", off until it is pressed, or the same words as
//! a check item in the tray menu (`autostart.rs`).
//!
//! AN EXISTING INSTALL KEEPS WHAT IT HAS. An install that registered itself
//! under 0.0.15 or older has been starting at sign-in and serving its
//! projects since; removing the entry on update would stop that without a
//! word, the same unannounced change the other way round. Both places show it
//! on, and turning it off is one press. So launch never adds an entry and
//! never removes one: it only rewrites an entry that is there (an entry
//! 0.0.3 and older wrote lacks `--autostart`, and would open the window at
//! every sign-in).
//!
//! The rule is pure and std-only so `scripts/test-rust.sh` runs it without
//! building the app; its tests also pin that nothing else writes the entry.

/// What launch does to the start-at-sign-in entry.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AtLaunch {
    /// Nothing: no entry is added, none is removed.
    Leave,
    /// Write the entry that is there again, with the current flag.
    Rewrite,
}

/// `registered`: whether Windows has the entry; `None` when it could not say.
/// Unknown is never a reason to write.
pub fn at_launch(registered: Option<bool>) -> AtLaunch {
    if registered == Some(true) { AtLaunch::Rewrite } else { AtLaunch::Leave }
}

#[cfg(test)]
mod tests {
    use super::{at_launch, AtLaunch};

    include!("source_pins.rs");

    #[test]
    fn a_fresh_install_does_not_start_at_sign_in() {
        assert_eq!(at_launch(Some(false)), AtLaunch::Leave);
        assert_eq!(at_launch(None), AtLaunch::Leave, "unknown writes nothing");
    }

    #[test]
    fn an_install_that_starts_at_sign_in_keeps_it() {
        assert_eq!(at_launch(Some(true)), AtLaunch::Rewrite);
    }

    /// One home for the entry: every `enable()`/`disable()` on the autostart
    /// manager is in `autostart.rs` — the launch rewrite (behind `at_launch`)
    /// and a person's press — and only that file takes the plugin's manager.
    /// The first-run marker is gone for good.
    #[test]
    fn only_autostart_rs_writes_the_entry() {
        let sources: Vec<(String, String)> = rust_sources().into_iter().map(|(file, source)| (file, code(&source))).collect();
        let home = sources.iter().find(|(file, _)| file == "autostart.rs").map(|(_, source)| source.as_str()).unwrap_or("");
        // Canary: the walk sees the home, and the home holds what is pinned.
        assert!(home.contains("fn on_launch("), "autostart.rs is walked and keeps the launch step");
        assert!(home.contains("at_launch(registered(app)) == AtLaunch::Rewrite"), "the launch write asks the rule");
        assert_eq!(home.matches(".enable()").count(), 2, "the launch rewrite and a press");
        assert_eq!(home.matches(".disable()").count(), 1, "a press");
        for (file, source) in &sources {
            if file == "autostart.rs" || file == "autostart_rules.rs" { continue; }
            assert!(!source.contains("tauri_plugin_autostart::ManagerExt"), "{file} takes the autostart manager");
            assert!(!source.contains("autolaunch()"), "{file} reaches the autostart manager");
            assert!(!source.contains("autostart-initialized"), "{file} brings back the first-run marker");
        }
        let lib = sources.iter().find(|(file, _)| file == "lib.rs").map(|(_, source)| source.as_str()).unwrap_or("");
        assert!(lib.contains("autostart::on_launch("), "launch goes through the rule");
    }
}
