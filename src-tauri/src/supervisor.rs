//! SUPERVISING ONE PROJECT'S DAEMON: start it, relay what it says, restart
//! it when it stops, stand by while something else serves it, and stop them
//! all on the way out.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). The lifecycle rules —
//! the refusal window, the quiet retry, standing by for a terminal
//! `flowviant`, never booting a distro that was shut down — change together
//! and apart from the window, the updater and the tray; `lib.rs` only starts
//! the supervisors at launch.

use serde_json::{json, Value};
use std::{io::{BufRead, BufReader}, process::Stdio, sync::{atomic::{AtomicBool, Ordering}, Arc}, thread, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_notification::NotificationExt;

use crate::bundled_daemon::ensure_binary;
use crate::connections::Connection;
use crate::daemon_output::{readable, stderr_tail};
use crate::machine_log::log_line;
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

/// How long a start may run before its exit counts as a stop rather than a
/// refusal. A daemon that will not serve (another project is bound to this repo,
/// the folder is not the one that was connected, a newer daemon holds the lock)
/// says so and exits within seconds.
const REFUSAL_WINDOW: Duration = Duration::from_secs(60);
/// How often a refused start is quietly tried again (the cause is usually
/// fixed by a person — a first commit, a folder put back — and nobody should
/// have to press Start after fixing it).
const REFUSAL_RETRY: Duration = Duration::from_secs(5 * 60);
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
/// but announced only once until it serves again.
fn supervise(app: AppHandle, connection: Connection, cancel: Arc<AtomicBool>) {
    let mut backoff = 2;
    let mut told = false;
    let mut first = true;
    let who = connection.name.clone().unwrap_or_else(|| connection.id.clone());
    loop {
        if cancel.load(Ordering::SeqCst) { break; }
        // The FIRST start may boot WSL: it is a launch, a Start or a switch
        // turned on — a person's intent. A RESTART never does: if the distro is
        // down (`wsl --shutdown`), wait for someone to bring it back.
        if !first && !distro_running(&connection.distro) {
            log_line(&app, &format!("{who}: {} is not running; waiting for it (not starting WSL)", connection.distro));
            if !wait_for_distro(&cancel, &connection.distro) { break; }
            log_line(&app, &format!("{who}: {} is running again", connection.distro));
        }
        first = false;
        // Something else already serves it here (a `flowviant` in a terminal):
        // starting now would take it over, so stand by until it is gone.
        if daemon_running(&connection.distro, &connection.id) == Some(true) {
            let mut free = false;
            while nap(&cancel, STANDBY_CHECK) {
                if daemon_running(&connection.distro, &connection.id) != Some(true) { free = true; break; }
            }
            if !free { break; }
        }
        let binary = match binary_path(&connection.distro) { Ok(path) => path, Err(_) => break };
        let result = wsl(&connection.distro, Some(&connection.dir), &[&binary, "--project", &connection.id, "--dir", &connection.dir, "--json-events"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn();
        let Ok(mut child) = result else {
            fail_project(&app, &connection.id, "Could not start wsl.exe", &mut told);
            break;
        };
        let started = Instant::now();
        log_line(&app, &format!("{who}: started the daemon in {}:{}", connection.distro, connection.dir));
        let stderr = child.stderr.take().map(stderr_tail);
        let mut displaced = false;
        let mut clean_stop = false;
        let mut stop_reason: Option<String> = None;
        let mut served = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(mut event) = serde_json::from_str::<Value>(&readable(&line)) {
                    match event.get("event").and_then(Value::as_str) {
                        Some("displaced") => displaced = true,
                        Some("stopped") => { clean_stop = true; stop_reason = event["reason"].as_str().map(str::to_string); }
                        Some("serving") => { served = true; told = false; backoff = 2; clear_failure(&app, &connection.id); }
                        _ => (),
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
        log_line(&app, &format!("{who}: daemon exited (code {}, after {}s, served: {served}, stop: {}{}){}",
            code.map_or("none".into(), |c| c.to_string()), started.elapsed().as_secs(),
            stop_reason.as_deref().unwrap_or(if clean_stop { "clean" } else { "none" }),
            if displaced { ", displaced" } else { "" },
            tail.last().map(|l| format!(" — {l}")).unwrap_or_default()));
        if cancel.load(Ordering::SeqCst) || displaced { break; }
        // WSL WENT AWAY (`wsl --shutdown`, or Windows stopping it): a person's
        // act, not a daemon failure — no notification, no restart that would
        // boot it again. The card says so; the project resumes when WSL does.
        if !distro_running(&connection.distro) {
            if let Ok(mut failures) = app.state::<Controller>().failures.lock() {
                failures.insert(connection.id.clone(), "WSL isn't running. This project starts again when WSL does — open a WSL terminal, or press Start.".into());
            }
            let _ = app.emit("status-changed", ());
            continue;
        }
        if clean_stop {
            // The server ended this box's part (removed from the project, its
            // credential revoked): final. Anything else that stopped it cleanly
            // was a SIGNAL — `flowviant stop`, or a `flowviant` started in a
            // terminal in the same repo, which takes over. It used to be final
            // too, so when that terminal closed the project sat offline with
            // the app open (Death Note - Roblox, 2026-09-25). Now: wait until
            // nothing else is serving it here, then serve it again.
            if matches!(stop_reason.as_deref(), Some("removed") | Some("credential-revoked")) { break; }
            log_line(&app, &format!("{who}: stopped by a signal; standing by until nothing else serves it"));
            let mut free = false;
            while nap(&cancel, STANDBY_CHECK) {
                if daemon_running(&connection.distro, &connection.id) != Some(true) { free = true; break; }
            }
            if !free { break; }
            continue;
        }
        let reason = tail.last().cloned()
            .or_else(|| status.ok().and_then(|s| s.code()).map(|code| format!("flowviant exited with code {code}")))
            .unwrap_or_else(|| "flowviant exited".into());
        if !served && started.elapsed() < REFUSAL_WINDOW {
            // Said once (told), then tried again quietly: the fix is usually a
            // person's (a first commit), and after it the project should come
            // back by itself.
            fail_project(&app, &connection.id, &reason, &mut told);
            log_line(&app, &format!("{who}: start refused; trying again in {}s", REFUSAL_RETRY.as_secs()));
            if !nap(&cancel, REFUSAL_RETRY) { break; }
            continue;
        }
        fail_project(&app, &connection.id, &reason, &mut told);
        for _ in 0..backoff * 10 { if cancel.load(Ordering::SeqCst) { break; } thread::sleep(Duration::from_millis(100)); }
        backoff = (backoff * 2).min(60);
    }
    if let Ok(mut jobs) = app.state::<Controller>().jobs.lock() {
        if jobs.get(&connection.id).is_some_and(|current| Arc::ptr_eq(current, &cancel)) { jobs.remove(&connection.id); }
    }
}

/// Record why a project's daemon is not running (the card shows it) and notify,
/// once, until it serves again.
fn fail_project(app: &AppHandle, id: &str, reason: &str, told: &mut bool) {
    if let Ok(mut failures) = app.state::<Controller>().failures.lock() { failures.insert(id.to_string(), reason.to_string()); }
    let event = json!({ "event": "stopped-unexpectedly", "projectId": id, "message": format!("flowviant stopped: {reason}") });
    if *told { let _ = app.emit("machine-event", &event); } else { announce(app, &event); *told = true; }
    let _ = app.emit("status-changed", ());
}

pub(crate) fn clear_failure(app: &AppHandle, id: &str) {
    if let Ok(mut failures) = app.state::<Controller>().failures.lock() { failures.remove(id); }
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
