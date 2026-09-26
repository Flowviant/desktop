//! UPDATING THE APP ITSELF: check, download, stage, and install when
//! nothing here is working (or when a person presses "Restart now").
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). The WHEN rule is pure and
//! lives in `updates.rs` (`can_auto_install`, tested with plain `rustc`); this
//! is its Tauri wiring — the manifest check, the staged installer, the local
//! status reads the rule is asked over, and the card's commands.

use serde_json::{json, Value};
use std::{collections::HashSet, thread, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::UpdaterExt;

use crate::connections::Connection;
use crate::machine_log::log_line;
use crate::supervisor::stop_daemons;
use crate::updates::{self, ServedProject};
use crate::wsl::{binary_path, run_wsl, running_distros};
use crate::Controller;

// ── Updating the app itself ──────────────────────────────────────────────────
//
// The app checks its manifest (`/dl/desktop/latest.json`, signed with the key in
// tauri.conf.json) a minute after it starts and every six hours, and downloads a
// newer installer in the background. It installs ON ITS OWN only when nothing is
// working on this computer: installing stops the daemons, and a turn cut off
// mid-way is lost work. "Nothing working" is measured per project whose daemon
// runs HERE, by that daemon itself (`busy` in its local `status --json`, see
// updates.rs); anything unmeasured counts as busy. The card also offers
// "Restart now" (a person's press, confirmed).
// The installer runs passively with /UPDATE (so the uninstall-cleanup hook skips)
// and relaunches the app with its own arguments, so a tray start stays in the tray.

// Hourly, and whenever the card is opened if the last check is older than
// UPDATE_CHECK_ON_OPEN. It was six hours at 0.0.9, so a release published just
// after an app started reached it only the next morning.
const UPDATE_CHECK_EVERY: Duration = Duration::from_secs(60 * 60);
const UPDATE_CHECK_ON_OPEN: Duration = Duration::from_secs(10 * 60);
const UPDATE_TICK: Duration = Duration::from_secs(10 * 60);

/// Check now unless a check ran within `fresh`; returns the staged version.
async fn check_now(app: &AppHandle, fresh: Duration) -> Result<Option<String>, String> {
    let staged = || app.state::<Controller>().pending_update.lock().ok().and_then(|p| p.as_ref().map(|(u, _)| u.version.clone()));
    if let Some(version) = staged() { return Ok(Some(version)); }
    {
        let controller = app.state::<Controller>();
        let mut checked = controller.update_checked.lock().map_err(|e| e.to_string())?;
        if checked.is_some_and(|at| at.elapsed() < fresh) { return Ok(None); }
        *checked = Some(Instant::now());
    }
    stage_update(app).await?;
    Ok(staged())
}

async fn stage_update(app: &AppHandle) -> Result<(), String> {
    let Some(update) = app.updater().map_err(|e| e.to_string())?.check().await.map_err(|e| e.to_string())? else { return Ok(()) };
    let bytes = update.download(|_, _| {}, || {}).await.map_err(|e| e.to_string())?;
    let version = update.version.clone();
    log_line(app, &format!("update {version} downloaded; installs when nothing is running here"));
    *app.state::<Controller>().pending_update.lock().map_err(|e| e.to_string())? = Some((update, bytes));
    let _ = app.emit("update-ready", json!({ "version": version }));
    Ok(())
}

/// True only when every project whose daemon RUNS HERE reports a MEASURED idle —
/// the rule is `updates::can_auto_install`; this only reads each running
/// distro's LOCAL `status --json` (the daemon's own `busy`). A project not
/// running on this computer has nothing here to cut off; anything unread or
/// unmeasured holds the install.
fn machine_idle(app: &AppHandle) -> bool {
    let rows = app.state::<Controller>().connections.lock().map(|rows| rows.clone()).unwrap_or_default();
    // A stopped distro runs nothing, and asking it would boot it.
    let running = running_distros();
    let distros: HashSet<String> = rows.iter().map(|row| row.distro.clone()).filter(|d| running.contains(d)).collect();
    let reads: Vec<Option<Vec<ServedProject>>> = distros.iter().map(|distro| {
        let binary = binary_path(distro).ok()?;
        let raw = run_wsl(distro, None, &[&binary, "status", "--json"]).ok()?;
        let value = serde_json::from_str::<Value>(&raw).ok()?;
        Some(served_projects(&value, &rows))
    }).collect();
    updates::can_auto_install(&reads)
}

/// The projects this app serves, as one status read measured them.
fn served_projects(status: &Value, rows: &[Connection]) -> Vec<ServedProject> {
    status["projects"].as_array().into_iter().flatten()
        .filter(|project| rows.iter().any(|row| project["id"] == json!(row.id)))
        .map(|project| ServedProject { running: project["running"] == json!(true), busy: project["busy"].as_bool() })
        .collect()
}

fn install_pending(app: &AppHandle) -> Result<(), String> {
    let staged = app.state::<Controller>().pending_update.lock().map_err(|e| e.to_string())?.take();
    let Some((update, bytes)) = staged else { return Err("No update is ready".into()) };
    log_line(app, &format!("installing update {}: stopping the daemons first", update.version));
    stop_daemons(app);
    // On Windows this starts the installer and exits the process.
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

pub(crate) fn update_loop(app: AppHandle) {
    if cfg!(debug_assertions) { return; }
    thread::spawn(move || {
        thread::sleep(Duration::from_secs(60));
        loop {
            let staged = || app.state::<Controller>().pending_update.lock().map(|p| p.is_some()).unwrap_or(false);
            if !staged() { let _ = tauri::async_runtime::block_on(check_now(&app, UPDATE_CHECK_EVERY)); }
            if staged() && machine_idle(&app) { let _ = install_pending(&app); }
            thread::sleep(UPDATE_TICK);
        }
    });
}

/// The card's version line and its "Check for updates". `force` is the press;
/// opening the card passes false and re-checks only after UPDATE_CHECK_ON_OPEN.
#[tauri::command]
pub async fn check_update(app: AppHandle, force: Option<bool>) -> Result<Option<String>, String> {
    if cfg!(debug_assertions) { return Ok(None); }
    check_now(&app, if force == Some(true) { Duration::ZERO } else { UPDATE_CHECK_ON_OPEN }).await
}

#[tauri::command]
pub fn app_version(app: AppHandle) -> String { app.package_info().version.to_string() }

#[tauri::command]
pub fn pending_update(app: AppHandle) -> Option<String> {
    app.state::<Controller>().pending_update.lock().ok().and_then(|p| p.as_ref().map(|(update, _)| update.version.clone()))
}

/// The card's "Restart now": a person chose to update while work may be running.
#[tauri::command(async)]
pub fn install_update(app: AppHandle) -> Result<(), String> { install_pending(&app) }

#[cfg(test)]
mod tests {
    use super::served_projects;
    use crate::connections::Connection;
    use crate::updates::ServedProject;
    use serde_json::json;

    /// The rule is asked only about projects THIS app serves, with `busy`
    /// kept unknown unless the daemon measured a boolean.
    #[test]
    fn a_status_read_becomes_the_rules_input() {
        let rows = vec![
            Connection { id: "a".into(), distro: "Ubuntu".into(), dir: "/a".into(), enabled: true, name: None },
            Connection { id: "b".into(), distro: "Ubuntu".into(), dir: "/b".into(), enabled: false, name: None },
        ];
        let status = json!({ "projects": [
            { "id": "a", "running": true, "busy": false },
            { "id": "b", "running": "yes", "busy": "no" },
            { "id": "terminal-only", "running": true, "busy": true },
        ] });
        assert_eq!(served_projects(&status, &rows), vec![
            ServedProject { running: true, busy: Some(false) },
            ServedProject { running: false, busy: None },
        ]);
        assert_eq!(served_projects(&json!({}), &rows), vec![]);
    }
}
