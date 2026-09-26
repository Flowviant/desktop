//! WHAT A SUPERVISOR DOES NEXT, as pure decisions (2026-09-26, SOLID
//! follow-up to F161).
//!
//! `supervisor.rs` starts wsl.exe, reads the daemon's events, sleeps and
//! notifies; every choice it makes between those acts is asked of this
//! module. The rules used to be written inline in the loop, between process
//! and thread I/O, so none of them had a test. They are std-only here, tested
//! without building the app: `rustc --edition 2021 --test src/supervisor_rules.rs`.
//!
//!   * a RESTART never boots WSL; only the first start (a person's intent) may;
//!   * something else measured serving the project here is waited out, and
//!     unknown is not "something else";
//!   * a daemon that exits inside the refusal window without ever serving was
//!     refused: said once, then tried again quietly every five minutes;
//!   * a daemon that did run is restarted after a backoff that doubles from 2 s
//!     to at most 60 s, back to 2 s once it serves;
//!   * removed from the project or its credential revoked is final; any other
//!     clean stop was a signal, and the project is served again once nothing
//!     else serves it;
//!   * WSL going away is a person's act: recorded on the card, never notified.

use std::time::Duration;

/// How long a start may run before its exit counts as a stop rather than a
/// refusal. A daemon that will not serve (another project is bound to this repo,
/// the folder is not the one that was connected, a newer daemon holds the lock)
/// says so and exits within seconds.
pub(crate) const REFUSAL_WINDOW: Duration = Duration::from_secs(60);
/// How often a refused start is quietly tried again (the cause is usually
/// fixed by a person — a first commit, a folder put back — and nobody should
/// have to press Start after fixing it).
pub(crate) const REFUSAL_RETRY: Duration = Duration::from_secs(5 * 60);
/// The first wait before restarting a daemon that ran and stopped, in seconds.
pub(crate) const BACKOFF_START_SECS: u64 = 2;
/// The longest wait before a restart, in seconds.
pub(crate) const BACKOFF_CAP_SECS: u64 = 60;

/// The card's line for a project whose WSL went away while it was served
/// (`trayModel.ts` shows the same words for a distro found stopped).
pub(crate) const WSL_STOPPED: &str = "WSL isn't running. This project starts again when WSL does — open a WSL terminal, or press Start.";

/// Whether this start must first wait for the distro to run again. Only the
/// FIRST start may boot WSL (a launch, a Start, a switch turned on); a restart
/// over a stopped distro waits. `distro_running` is asked only for a restart.
pub(crate) fn waits_for_distro(first_start: bool, distro_running: impl FnOnce() -> bool) -> bool {
    !first_start && !distro_running()
}

/// Whether something else (a `flowviant` in a terminal) serves the project
/// here, so starting now would take it over. Only a measured `true` stands by;
/// an unreadable status starts.
pub(crate) fn serving_elsewhere(measured: Option<bool>) -> bool {
    measured == Some(true)
}

/// Removed from the project, or its credential revoked: the server ended this
/// box's part, and the supervisor stops for good.
pub(crate) fn final_stop(reason: Option<&str>) -> bool {
    matches!(reason, Some("removed") | Some("credential-revoked"))
}

/// What one daemon run said on stdout, as far as the supervisor cares.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Run {
    /// Another box took the machine.
    pub(crate) displaced: bool,
    /// The daemon said `stopped`.
    pub(crate) clean_stop: bool,
    /// The `reason` on its last `stopped` event.
    pub(crate) stop_reason: Option<String>,
    /// It said `serving` at least once.
    pub(crate) served: bool,
}

impl Run {
    /// Take one event (`event` and `reason` fields); true when it is
    /// `serving`, which is when the supervisor forgets the failure it told.
    pub(crate) fn observe(&mut self, kind: Option<&str>, reason: Option<&str>) -> bool {
        match kind {
            Some("displaced") => self.displaced = true,
            Some("stopped") => { self.clean_stop = true; self.stop_reason = reason.map(str::to_string); }
            Some("serving") => { self.served = true; return true; }
            _ => (),
        }
        false
    }
}

/// What the supervisor carries across runs: the restart backoff and whether
/// the current failure was already notified.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Supervision {
    pub(crate) backoff_secs: u64,
    pub(crate) told: bool,
}

impl Default for Supervision {
    fn default() -> Self { Supervision { backoff_secs: BACKOFF_START_SECS, told: false } }
}

impl Supervision {
    /// The daemon serves: the next failure is news again and the backoff
    /// starts over.
    pub(crate) fn served(&mut self) {
        self.told = false;
        self.backoff_secs = BACKOFF_START_SECS;
    }

    /// A failure is notified once until the daemon serves again: true the
    /// first time (notify), false after (relay to the window only).
    pub(crate) fn announce_once(&mut self) -> bool {
        !std::mem::replace(&mut self.told, true)
    }

    /// The wait before this restart, in seconds; the next one doubles, capped.
    pub(crate) fn backoff_then_double(&mut self) -> u64 {
        let wait = self.backoff_secs;
        self.backoff_secs = next_backoff(wait);
        wait
    }
}

/// The backoff after `secs`: doubled, at most `BACKOFF_CAP_SECS`.
pub(crate) fn next_backoff(secs: u64) -> u64 {
    (secs * 2).min(BACKOFF_CAP_SECS)
}

/// What the supervisor does once a daemon run has exited.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Next {
    /// Stop supervising: cancelled, displaced, removed or revoked.
    Quit,
    /// WSL went away: record `WSL_STOPPED` on the card, no notification, and
    /// start over (the restart waits for the distro, never boots it).
    WslStopped,
    /// Stopped by a signal: stand by until nothing else serves it, then serve.
    StandBy,
    /// Refused: say it once, retry quietly after `REFUSAL_RETRY`.
    RetryRefused,
    /// It ran and stopped: say it once, restart after the backoff.
    Restart,
}

/// The decision after a run. `distro_running` is asked only when the run was
/// neither cancelled nor displaced, and `ran_for` only for an exit that was
/// not a clean stop, in the order the loop always asked them.
pub(crate) fn after_exit(cancelled: bool, run: &Run, distro_running: impl FnOnce() -> bool, ran_for: impl FnOnce() -> Duration) -> Next {
    if cancelled || run.displaced { return Next::Quit; }
    if !distro_running() { return Next::WslStopped; }
    if run.clean_stop {
        return if final_stop(run.stop_reason.as_deref()) { Next::Quit } else { Next::StandBy };
    }
    if !run.served && ran_for() < REFUSAL_WINDOW { Next::RetryRefused } else { Next::Restart }
}

/// Why a daemon that stopped on its own stopped: its last readable stderr
/// line, else its exit code, else that it exited.
pub(crate) fn exit_reason(last_stderr: Option<&str>, code: Option<i32>) -> String {
    last_stderr.map(str::to_string)
        .or_else(|| code.map(|code| format!("flowviant exited with code {code}")))
        .unwrap_or_else(|| "flowviant exited".into())
}

/// The `stopped-unexpectedly` event's words for `reason`.
pub(crate) fn stopped_message(reason: &str) -> String {
    format!("flowviant stopped: {reason}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    include!("source_pins.rs");

    fn run(kinds: &[(&str, Option<&str>)]) -> Run {
        let mut run = Run::default();
        for (kind, reason) in kinds { run.observe(Some(kind), *reason); }
        run
    }

    const QUICK: Duration = Duration::from_secs(3);
    const LONG: Duration = Duration::from_secs(61);

    #[test]
    fn only_the_first_start_may_boot_wsl() {
        let asked = Cell::new(false);
        assert!(!waits_for_distro(true, || { asked.set(true); false }), "a person's start boots it");
        assert!(!asked.get(), "the first start does not even ask");
        assert!(waits_for_distro(false, || false), "a restart over a stopped distro waits");
        assert!(!waits_for_distro(false, || true));
    }

    #[test]
    fn only_a_measured_serving_daemon_is_waited_out() {
        assert!(serving_elsewhere(Some(true)));
        assert!(!serving_elsewhere(Some(false)));
        assert!(!serving_elsewhere(None), "unknown gates nothing");
    }

    #[test]
    fn events_fill_the_run() {
        let mut fresh = Run::default();
        assert!(fresh.observe(Some("serving"), None), "serving clears the failure");
        assert!(!fresh.observe(Some("limit-hit"), None));
        assert!(!fresh.observe(None, Some("removed")), "an event with no kind changes nothing");
        assert_eq!(fresh, Run { served: true, ..Run::default() });
        assert_eq!(run(&[("displaced", None)]), Run { displaced: true, ..Run::default() });
        assert_eq!(run(&[("stopped", Some("removed"))]), Run { clean_stop: true, stop_reason: Some("removed".into()), ..Run::default() });
        // The LAST stopped event's reason is the reason, absent included.
        assert_eq!(run(&[("stopped", Some("removed")), ("stopped", None)]).stop_reason, None);
    }

    #[test]
    fn cancelled_or_displaced_quits_before_anything_is_asked() {
        let never = || -> bool { panic!("distro asked") };
        let no_clock = || -> Duration { panic!("clock asked") };
        assert_eq!(after_exit(true, &Run::default(), never, no_clock), Next::Quit);
        assert_eq!(after_exit(false, &run(&[("displaced", None)]), never, no_clock), Next::Quit);
    }

    #[test]
    fn wsl_going_away_is_recorded_not_restarted() {
        let no_clock = || -> Duration { panic!("clock asked") };
        assert_eq!(after_exit(false, &Run::default(), || false, no_clock), Next::WslStopped);
        assert_eq!(after_exit(false, &run(&[("stopped", Some("removed"))]), || false, no_clock), Next::WslStopped, "WSL is asked before the stop reason");
    }

    #[test]
    fn removed_and_revoked_are_final_any_other_clean_stop_stands_by() {
        let no_clock = || -> Duration { panic!("clock asked") };
        assert_eq!(after_exit(false, &run(&[("stopped", Some("removed"))]), || true, no_clock), Next::Quit);
        assert_eq!(after_exit(false, &run(&[("stopped", Some("credential-revoked"))]), || true, no_clock), Next::Quit);
        assert_eq!(after_exit(false, &run(&[("stopped", Some("signal"))]), || true, no_clock), Next::StandBy);
        assert_eq!(after_exit(false, &run(&[("stopped", None)]), || true, no_clock), Next::StandBy);
        assert_eq!(after_exit(false, &run(&[("serving", None), ("stopped", Some("takeover"))]), || true, no_clock), Next::StandBy);
        assert!(final_stop(Some("removed")) && final_stop(Some("credential-revoked")));
        assert!(!final_stop(None) && !final_stop(Some("Removed")) && !final_stop(Some("")));
    }

    #[test]
    fn a_quick_exit_that_never_served_is_a_refusal() {
        assert_eq!(after_exit(false, &Run::default(), || true, || QUICK), Next::RetryRefused);
        assert_eq!(after_exit(false, &Run::default(), || true, || REFUSAL_WINDOW - Duration::from_millis(1)), Next::RetryRefused);
        assert_eq!(after_exit(false, &Run::default(), || true, || REFUSAL_WINDOW), Next::Restart, "the window is exclusive");
        assert_eq!(after_exit(false, &Run::default(), || true, || LONG), Next::Restart);
        assert_eq!(after_exit(false, &run(&[("serving", None)]), || true, || QUICK), Next::Restart, "a daemon that served was not refused");
        assert_eq!(REFUSAL_RETRY, Duration::from_secs(300));
    }

    #[test]
    fn the_backoff_doubles_from_two_to_sixty_and_serving_resets_it() {
        let mut state = Supervision::default();
        let waits: Vec<u64> = (0..8).map(|_| state.backoff_then_double()).collect();
        assert_eq!(waits, vec![2, 4, 8, 16, 32, 60, 60, 60]);
        state.served();
        assert_eq!(state.backoff_then_double(), 2);
        assert_eq!(next_backoff(45), 60);
    }

    #[test]
    fn a_failure_is_announced_once_until_it_serves() {
        let mut state = Supervision::default();
        assert!(state.announce_once());
        assert!(!state.announce_once());
        assert!(!state.announce_once());
        state.served();
        assert!(state.announce_once());
    }

    #[test]
    fn the_failure_words() {
        assert_eq!(exit_reason(Some("error: no git repository"), Some(1)), "error: no git repository");
        assert_eq!(exit_reason(None, Some(3)), "flowviant exited with code 3");
        assert_eq!(exit_reason(None, Some(0)), "flowviant exited with code 0", "a zero code is still the code (the sign-in says it too)");
        assert_eq!(exit_reason(None, None), "flowviant exited");
        assert_eq!(stopped_message("flowviant exited"), "flowviant stopped: flowviant exited");
        assert!(WSL_STOPPED.starts_with("WSL isn't running."));
    }

    /// The source half: the loop asks these rules, the sign-in (`connect.rs`)
    /// words a login that ended with the same `exit_reason`, and none of them
    /// is written a second time anywhere in `src/`. The needles are the
    /// rules' own shapes (`Some("credential-revoked")`, a doubling clamped by
    /// `.min(`), so a status read of the reason or an unrelated clamp to 60
    /// is not a copy.
    #[test]
    fn the_loop_asks_these_rules() {
        let supervisor = code(include_str!("supervisor.rs"));
        for call in ["supervisor_rules::waits_for_distro(first,", "supervisor_rules::serving_elsewhere(", ".observe(", "supervisor_rules::after_exit(",
            "supervisor_rules::exit_reason(", "supervisor_rules::stopped_message(", "supervisor_rules::WSL_STOPPED", ".announce_once()", ".backoff_then_double()", ".served()"] {
            assert!(supervisor.contains(call), "supervisor.rs no longer asks {call}");
        }
        assert!(code(include_str!("connect.rs")).contains("supervisor_rules::exit_reason("), "connect.rs no longer asks exit_reason");
        let home = code(include_str!("supervisor_rules.rs"));
        let banned = ["Some(\"credential-revoked\")", "* 2).min(", ".min(BACKOFF_CAP_SECS)", "const REFUSAL_WINDOW", "const REFUSAL_RETRY", "WSL isn't running", "flowviant stopped: ", "flowviant exited"];
        for needle in banned { assert!(home.contains(needle), "canary: the home no longer holds {needle}"); }
        let sources = rust_sources();
        assert!(sources.len() >= 10 && sources.iter().any(|(name, _)| name == "supervisor.rs"), "the walk found src/");
        for (name, source) in sources.iter().filter(|(name, _)| name != "supervisor_rules.rs") {
            let source = code(source);
            for needle in banned {
                assert!(!source.contains(needle), "{name} writes {needle} again");
            }
        }
    }

    /// Who holds a supervisor's state: only `supervisor.rs` locks the job and
    /// failure maps; everyone else calls `cancel`, `failure` or `clear_failure`.
    /// Matched with whitespace gone too, so a lock taken on the next line is
    /// the same lock, and `try_lock` is a lock.
    #[test]
    fn only_the_supervisor_touches_its_maps() {
        let squeezed = |source: &str| code(source).chars().filter(|c| !c.is_whitespace()).collect::<String>();
        let sources = rust_sources();
        let supervisor = squeezed(&sources.iter().find(|(name, _)| name == "supervisor.rs").expect("supervisor.rs").1);
        for map in [".jobs.", ".failures."] {
            assert!(supervisor.contains(&format!("{map}lock()")), "canary: supervisor.rs no longer holds {map}lock()");
            for (name, source) in sources.iter().filter(|(name, _)| name != "supervisor.rs" && name != "supervisor_rules.rs") {
                let source = squeezed(source);
                for lock in ["lock()", "try_lock()"] {
                    assert!(!source.contains(&format!("{map}{lock}")), "{name} takes {map}{lock} by hand again");
                }
            }
        }
    }

    /// `source_pins.rs`'s stripper, tested once here rather than in every
    /// module that includes it.
    #[test]
    fn the_stripper_keeps_code_and_literals() {
        assert_eq!(code("a // gone\nb"), "a \nb");
        assert_eq!(code("a /* x /* nested */ y */ b"), "a   b");
        assert_eq!(code("let u = \"https://x\"; // c"), "let u = \"https://x\"; ");
        assert_eq!(code("let s = \"say \\\"/*\\\" \"; t"), "let s = \"say \\\"/*\\\" \"; t");
        assert_eq!(code("let q = '\"'; // c\nlet r = '\\''; x"), "let q = '\"'; \nlet r = '\\''; x");
        assert_eq!(code("fn f<'a>(s: &'a str) {} // c"), "fn f<'a>(s: &'a str) {} ");
        assert_eq!(code("let p = r#\"a // \"b\" \"#; // c"), "let p = r#\"a // \"b\" \"#; ");
        assert_eq!(code("/// doc\n//! inner\nfn x() {}"), "\n\nfn x() {}");
        assert_eq!(code("error // c"), "error ");
    }
}
