//! WHEN THE APP MAY INSTALL AN UPDATE ON ITS OWN (2026-09-26, SOLID F034).
//!
//! Installing stops every daemon this app supervises, and a turn cut off
//! mid-way is lost work. The rule used to read the SERVER's list of working
//! agents (`status --json --remote`), which cannot see a Workbench turn, a
//! ship or a deploy running on this box — so an update could install over
//! any of them. The daemon now measures itself (`machineBusy` in the CLI's
//! fleet.mjs, the same predicate its own self-update asks) and reports it in
//! its LOCAL `status --json` as each project's `busy`.
//!
//! The rule is pure and std-only so it is tested on any box without building
//! the app (`rustc --edition 2021 --test src/updates.rs`):
//!   * a project whose daemon is not running here has nothing here to cut off;
//!   * a running one must report a MEASURED idle (`busy: false`) — absent,
//!     null (an older daemon, a stale reading) or true all hold the install;
//!   * a distro whose status could not be read at all holds it too.
//! Unknown is never idle. The card's "Restart now" is a person's press and is
//! not gated by this rule (`install_update` → `install_pending`, self_update.rs).

/// One project this app serves, as its daemon's local `status --json` said it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServedProject {
    /// `running` was exactly `true`.
    pub running: bool,
    /// `busy`: `Some` only for a boolean the daemon measured; `None` = unknown.
    pub busy: Option<bool>,
}

/// One entry per running distro: `None` when its status could not be read.
pub fn can_auto_install(reads: &[Option<Vec<ServedProject>>]) -> bool {
    reads.iter().all(|read| match read {
        None => false,
        Some(projects) => projects.iter().all(|p| !p.running || p.busy == Some(false)),
    })
}

#[cfg(test)]
mod tests {
    use super::{can_auto_install, ServedProject};

    const IDLE: ServedProject = ServedProject { running: true, busy: Some(false) };
    const WORKING: ServedProject = ServedProject { running: true, busy: Some(true) };
    const UNKNOWN: ServedProject = ServedProject { running: true, busy: None };
    const STOPPED: ServedProject = ServedProject { running: false, busy: None };

    #[test]
    fn a_measured_idle_everywhere_installs() {
        assert!(can_auto_install(&[Some(vec![IDLE, IDLE]), Some(vec![IDLE])]));
        assert!(can_auto_install(&[]), "no running distro runs nothing");
        assert!(can_auto_install(&[Some(vec![])]));
    }

    /// A Workbench turn, a ship and a deploy all reach the tray as `busy: true`
    /// (the CLI's machineBusy.test.mjs pins that they are counted); any one of
    /// them, on any served project, holds the install.
    #[test]
    fn work_the_agent_list_never_showed_holds_the_install() {
        assert!(!can_auto_install(&[Some(vec![IDLE, WORKING])]));
        assert!(!can_auto_install(&[Some(vec![IDLE]), Some(vec![WORKING])]));
    }

    #[test]
    fn unknown_is_never_idle() {
        assert!(!can_auto_install(&[Some(vec![UNKNOWN])]), "an older daemon or a stale reading");
        assert!(!can_auto_install(&[None]), "a status that could not be read");
        assert!(!can_auto_install(&[Some(vec![IDLE]), None]));
    }

    #[test]
    fn a_daemon_not_running_here_has_nothing_to_cut_off() {
        assert!(can_auto_install(&[Some(vec![STOPPED, IDLE])]));
        assert!(can_auto_install(&[Some(vec![ServedProject { running: false, busy: Some(true) }])]));
    }

    /// The source half: the loop asks this rule over the LOCAL status, and the
    /// card's "Restart now" stays an ungated, explicit install. The wiring
    /// moved out of `lib.rs` into `self_update.rs` (SOLID F161); the pin moved
    /// with it, and `lib.rs` is held to not growing a second copy.
    #[test]
    fn the_loop_asks_this_rule_and_restart_now_is_not_gated() {
        let wiring = include_str!("self_update.rs");
        let idle = wiring.find("fn machine_idle(").expect("machine_idle");
        let end = wiring[idle..].find("\n}\n").expect("machine_idle end") + idle;
        let body = &wiring[idle..end];
        assert!(body.contains("updates::can_auto_install(&reads)"));
        assert!(body.contains("\"status\", \"--json\"]"));
        assert!(!body.contains("--remote"), "the server's agent list is not the measurement");
        assert!(wiring.contains("if staged() && machine_idle(&app) { let _ = install_pending(&app); }"));
        assert!(wiring.contains("fn install_update(app: AppHandle) -> Result<(), String> { install_pending(&app) }"));
        let lib = include_str!("lib.rs");
        for moved in ["fn machine_idle(", "fn install_pending(", "fn update_loop(", "can_auto_install"] {
            assert!(!lib.contains(moved), "lib.rs grew {moved} back");
        }
    }
}
