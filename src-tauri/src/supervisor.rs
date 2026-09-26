//! SUPERVISING ONE PROJECT'S DAEMON: start it, relay what it says, restart
//! it when it stops, stand by while something else serves it, and stop them
//! all on the way out.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). The lifecycle rules —
//! the refusal window, the quiet retry, the backoff, standing by for a
//! terminal `flowviant`, never booting a distro that was shut down, which
//! stops are final — change together and apart from the window, the updater
//! and the tray; `lib.rs` only starts the supervisors at launch. The rules
//! themselves are pure decisions in `supervisor_rules` (std-only, tested with
//! plain rustc); this file is the I/O that asks them. It is also the one
//! holder of a supervisor's state: the job map (`start_one`, `cancel`,
//! `stop_daemons`) and the failure map (`failure`, `clear_failure`).

use serde_json::{json, Value};
use std::{io::{BufRead, BufReader}, process::Stdio, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::bundled_daemon::ensure_binary;
use crate::connections::Connection;
use crate::daemon_output::{readable, stderr_tail};
use crate::machine_log::log_line;
use crate::supervisor_rules::{self, Next, Run, Supervision};
use crate::tray::tray_icon;
use crate::wsl::{binary_path, distro_running, run_wsl, wsl};
use crate::{notifications, Controller};

/// Relay a machine event to the window, and notify when `notifications`
/// says it is one worth a native notification.
fn announce(app: &AppHandle, event: &Value) {
    let _ = app.emit("machine-event", event);
    let kind = event.get("event").and_then(Value::as_str).unwrap_or("");
    if let Some(notice) = notifications::notification_for_event(kind, event.get("message").and_then(Value::as_str)) {
        let _ = app.notification().builder().title("Flowviant machine").body(notice.body).show();
        if notice.attention { tray_icon(app, "attention"); }
    } else if kind == "serving" { tray_icon(app, "serving"); }
}

/// How often a signalled-away project is checked for being free again.
const STANDBY_CHECK: Duration = Duration::from_secs(15);

/// How often a project whose WSL went away checks for it coming back.
const WSL_CHECK: Duration = Duration::from_secs(30);

/// Wait, without booting it, until `distro` runs again; false if cancelled.
fn wait_for_distro(cancel: &AtomicBool, distro: &str) -> bool {
    loop {
        if distro_running(distro) { return true; }
        if !nap(cancel, WSL_CHECK) { return false; }
    }
}

/// Sleep in small steps; false when the supervisor was cancelled meanwhile.
fn nap(cancel: &AtomicBool, total: Duration) -> bool {
    let until = Instant::now() + total;
    while Instant::now() < until {
        if cancel.load(Ordering::SeqCst) { return false; }
        thread::sleep(Duration::from_millis(200));
    }
    !cancel.load(Ordering::SeqCst)
}

/// Whether a daemon (any, e.g. one started in a terminal) is serving this
/// project on this box right now, as `flowviant status --json` measures it.
fn daemon_running(distro: &str, id: &str) -> Option<bool> {
    // Asking would boot a stopped distro; a stopped distro runs no daemon.
    if !distro_running(distro) { return Some(false); }
    let binary = binary_path(distro).ok()?;
    let raw = run_wsl(distro, None, &[&binary, "status", "--json"]).ok()?;
    let value: Value = serde_json::from_str(&raw).ok()?;
    value["projects"].as_array()?.iter().find(|p| p["id"] == json!(id)).and_then(|p| p["running"].as_bool())
}

/// Supervise one project's daemon: restart it when it stops, but never loop on a
/// refusal and never repeat the notification.
///
/// This used to restart ANY exit that did not say "stopped" and notify each time,
/// with the daemon's stderr thrown away. A daemon refusing to start exits at
/// once, the next start refuses again, so a person got "The daemon stopped
/// unexpectedly" up to once a minute forever, with no reason (2026-09-25).
/// Now its last stderr line is the reason; a start that ends inside
/// REFUSAL_WINDOW without ever serving is not retried (said once, in its words;
/// the card offers Start); and a daemon that did run is restarted with backoff
/// but announced only once until it serves again. Every such decision is
/// `supervisor_rules`'.
fn supervise(app: AppHandle, connection: Connection, cancel: Arc<AtomicBool>) {
    let mut state = Supervision::default();
    let mut first = true;
    let who = connection.name.clone().unwrap_or_else(|| connection.id.clone());
    loop {
        if cancel.load(Ordering::SeqCst) { break; }
        // The FIRST start may boot WSL: it is a launch, a Start or a switch
        // turned on — a person's intent. A RESTART never does: if the distro is
        // down (`wsl --shutdown`), wait for someone to bring it back.
        if supervisor_rules::waits_for_distro(first, || distro_running(&connection.distro)) {
            log_line(&app, &format!("{who}: {} is not running; waiting for it (not starting WSL)", connection.distro));
            if !wait_for_distro(&cancel, &connection.distro) { break; }
            log_line(&app, &format!("{who}: {} is running again", connection.distro));
        }
        first = false;
        // Something else already serves it here (a `flowviant` in a terminal):
        // starting now would take it over, so stand by until it is gone.
        if supervisor_rules::serving_elsewhere(daemon_running(&connection.distro, &connection.id)) {
            if !stand_by(&cancel, &connection) { break; }
        }
        let binary = match binary_path(&connection.distro) { Ok(path) => path, Err(_) => break };
        let result = wsl(&connection.distro, Some(&connection.dir), &[&binary, "--project", &connection.id, "--dir", &connection.dir, "--json-events"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let Ok(mut child) = result else {
            fail_project(&app, &connection.id, "Could not start wsl.exe", state.announce_once());
            break;
        };
        let started = Instant::now();
        log_line(&app, &format!("{who}: started the daemon in {}:{}", connection.distro, connection.dir));
        let stderr = child.stderr.take().map(stderr_tail);
        let mut run = Run::default();
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(mut event) = serde_json::from_str::<Value>(&readable(&line)) {
                    if run.observe(event.get("event").and_then(Value::as_str), event["reason"].as_str()) {
                        state.served();
                        clear_failure(&app, &connection.id);
                    }
                    event["projectId"] = json!(connection.id);
                    announce(&app, &event);
                }
            }
        }
        let status = child.wait();
        let tail = stderr.and_then(|handle| handle.join().ok()).unwrap_or_default();
        let _ = app.emit("status-changed", ());
        let code = status.as_ref().ok().and_then(|s| s.code());
        log_line(&app, &format!("{who}: daemon exited (code {}, after {}s, served: {}, stop: {}{}){}",
            code.map_or("none".into(), |c| c.to_string()), started.elapsed().as_secs(), run.served,
            run.stop_reason.as_deref().unwrap_or(if run.clean_stop { "clean" } else { "none" }),
            if run.displaced { ", displaced" } else { "" },
            tail.last().map(|l| format!(" — {l}")).unwrap_or_default()));
        match supervisor_rules::after_exit(cancel.load(Ordering::SeqCst), &run, || distro_running(&connection.distro), || started.elapsed()) {
            Next::Quit => break,
            // WSL WENT AWAY (`wsl --shutdown`, or Windows stopping it): a person's
            // act, not a daemon failure — no notification, no restart that would
            // boot it again. The card says so; the project resumes when WSL does.
            Next::WslStopped => {
                record_failure(&app, &connection.id, supervisor_rules::WSL_STOPPED);
                let _ = app.emit("status-changed", ());
                continue;
            }
            // Removed from the project or its credential revoked was `Quit`.
            // Anything else that stopped it cleanly was a SIGNAL — `flowviant
            // stop`, or a `flowviant` started in a terminal in the same repo,
            // which takes over. It used to be final too, so when that terminal
            // closed the project sat offline with the app open (Death Note -
            // Roblox, 2026-09-25). Now: wait until nothing else is serving it
            // here, then serve it again.
            Next::StandBy => {
                log_line(&app, &format!("{who}: stopped by a signal; standing by until nothing else serves it"));
                if !stand_by(&cancel, &connection) { break; }
                continue;
            }
            // Said once (told), then tried again quietly: the fix is usually a
            // person's (a first commit), and after it the project should come
            // back by itself.
            Next::RetryRefused => {
                let reason = supervisor_rules::exit_reason(tail.last().map(String::as_str), code);
                fail_project(&app, &connection.id, &reason, state.announce_once());
                log_line(&app, &format!("{who}: start refused; trying again in {}s", supervisor_rules::REFUSAL_RETRY.as_secs()));
                if !nap(&cancel, supervisor_rules::REFUSAL_RETRY) { break; }
                continue;
            }
            Next::Restart => {
                let reason = supervisor_rules::exit_reason(tail.last().map(String::as_str), code);
                fail_project(&app, &connection.id, &reason, state.announce_once());
                for _ in 0..state.backoff_then_double() * 10 { if cancel.load(Ordering::SeqCst) { break; } thread::sleep(Duration::from_millis(100)); }
            }
        }
    }
    if let Ok(mut jobs) = app.state::<Controller>().jobs.lock() {
        if jobs.get(&connection.id).is_some_and(|current| Arc::ptr_eq(current, &cancel)) { jobs.remove(&connection.id); }
    }
}

/// Wait while something else serves the project here; true once it is free,
/// false if the supervisor was cancelled meanwhile.
fn stand_by(cancel: &AtomicBool, connection: &Connection) -> bool {
    while nap(cancel, STANDBY_CHECK) {
        if !supervisor_rules::serving_elsewhere(daemon_running(&connection.distro, &connection.id)) { return true; }
    }
    false
}

/// Record why a project's daemon is not running (the card shows it) and
/// notify when `announce_it` (`Supervision::announce_once`: once, until it
/// serves again); otherwise only relay it to the window.
fn fail_project(app: &AppHandle, id: &str, reason: &str, announce_it: bool) {
    record_failure(app, id, reason);
    let event = json!({ "event": "stopped-unexpectedly", "projectId": id, "message": supervisor_rules::stopped_message(reason) });
    if announce_it { announce(app, &event); } else { let _ = app.emit("machine-event", &event); }
    let _ = app.emit("status-changed", ());
}

fn record_failure(app: &AppHandle, id: &str, reason: &str) {
    if let Ok(mut failures) = app.state::<Controller>().failures.lock() { failures.insert(id.to_string(), reason.to_string()); }
}

/// Why this project's daemon is not running, as the supervisor last saw it.
pub(crate) fn failure(app: &AppHandle, id: &str) -> Option<String> {
    app.state::<Controller>().failures.lock().ok().and_then(|failures| failures.get(id).cloned())
}

pub(crate) fn clear_failure(app: &AppHandle, id: &str) {
    if let Ok(mut failures) = app.state::<Controller>().failures.lock() { failures.remove(id); }
}

/// End this project's supervisor, if one runs: it stops at its next check and
/// never restarts. The daemon itself is stopped by the caller.
pub(crate) fn cancel(app: &AppHandle, id: &str) -> Result<(), String> {
    if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(id) { cancel.store(true, Ordering::SeqCst); }
    Ok(())
}

pub(crate) fn start_one(app: &AppHandle, connection: Connection) -> Result<(), String> {
    ensure_binary(app, &connection.distro)?;
    let controller = app.state::<Controller>();
    let mut jobs = controller.jobs.lock().map_err(|e| e.to_string())?;
    if jobs.contains_key(&connection.id) { return Ok(()); }
    let cancel = Arc::new(AtomicBool::new(false));
    jobs.insert(connection.id.clone(), cancel.clone());
    let handle = app.clone();
    thread::spawn(move || supervise(handle, connection, cancel));
    Ok(())
}

/// Stop every daemon this app started, the way `flowviant stop` does.
pub(crate) fn stop_daemons(app: &AppHandle) {
    let rows = app.state::<Controller>().connections.lock().map(|rows| rows.clone()).unwrap_or_default();
    if let Ok(mut jobs) = app.state::<Controller>().jobs.lock() {
        for (_, cancel) in jobs.drain() { cancel.store(true, Ordering::SeqCst); }
    }
    for row in rows {
        if let Ok(binary) = binary_path(&row.distro) { let _ = run_wsl(&row.distro, None, &[&binary, "stop", "--project", &row.id]); }
    }
}
