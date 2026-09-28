//! WHAT THE CARD SHOWS: every project this box serves, as its daemons
//! report it, with the app's own facts merged in.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). One read per RUNNING
//! distro (never one that is stopped — asking would boot it), each row
//! stamped with its distro, daemon version, the supervisor's last stop
//! reason and the card's switch (and, on a row finishing a deploy, whether
//! the supervisor waits to restart it); a saved project in a stopped distro is
//! listed from the saved connection, marked `wslStopped`. The read also
//! learns names and projects bound from a terminal into the saved list.

use serde_json::{json, Value};
use std::collections::HashSet;
use tauri::{AppHandle, Manager};

use crate::connections::{save_connections, Connection};
use crate::supervisor::{failure, standing_by};
use crate::supervisor_rules;
use crate::setup;
use crate::wsl::{binary_path, run_wsl, running_distros};
use crate::Controller;

fn status_args(binary: &str, remote: bool, refresh_auth: bool) -> Vec<&str> {
    let mut args = vec![binary, "status", "--json"];
    if remote { args.push("--remote"); }
    if refresh_auth { args.push("--refresh-auth"); }
    args
}

#[tauri::command(async)]
pub fn status(app: AppHandle, remote: Option<bool>, refresh_auth: Option<bool>) -> Result<Value, String> {
    // ONLY DISTROS ALREADY RUNNING. Every `wsl.exe -d <distro>` boots that
    // distro, and this runs every 30 s: it is what started WSL again after the
    // owner's `wsl --shutdown`. A project in a stopped distro is listed from the
    // saved connection below instead, marked `wslStopped`.
    let running = running_distros();
    let distros: HashSet<String> = running.clone();
    let mut projects = Vec::new();
    let mut runtimes = Vec::new();
    let mut cli = Vec::new();
    let mut version = Value::Null;
    for distro in distros {
        let remote_raw = if remote == Some(true) {
            binary_path(&distro).ok().and_then(|binary| run_wsl(&distro, None, &status_args(&binary, true, refresh_auth == Some(true))).ok())
                .or_else(|| run_wsl(&distro, None, &status_args("flowviant", true, refresh_auth == Some(true))).ok())
                .filter(|raw| serde_json::from_str::<Value>(raw).is_ok())
        } else { None };
        let raw = remote_raw.or_else(|| binary_path(&distro).ok().and_then(|binary| run_wsl(&distro, None, &status_args(&binary, false, refresh_auth == Some(true))).ok())
            .or_else(|| run_wsl(&distro, None, &status_args("flowviant", false, refresh_auth == Some(true))).ok()));
        if let Some(raw) = raw {
            if let Ok(mut value) = serde_json::from_str::<Value>(&raw) {
                let distro_version = value["version"].clone();
                if version.is_null() { version = value["version"].clone(); }
                if runtimes.is_empty() { runtimes = value["runtimes"].as_array().cloned().unwrap_or_default(); }
                cli.push(json!({"distro":distro, "runtimes":value["runtimes"]}));
                if let Some(rows) = value["projects"].as_array_mut() {
                    for row in rows {
                        row["distro"] = json!(distro);
                        row["daemonVersion"] = distro_version.clone();
                        // A daemon finishing a deploy (its lock's mark): say whether
                        // this app waits to start it again after (owner ruling 22).
                        if let Some(id) = row["id"].as_str().map(str::to_string) {
                            if let Some(after) = supervisor_rules::restarts_after_drain(row["draining"].as_str(), || standing_by(&app, &id)) {
                                row["standingBy"] = json!(after);
                            }
                        }
                        if row["running"] != json!(true) {
                            if let Some(reason) = row["id"].as_str().and_then(|id| failure(&app, id)) {
                                row["stopReason"] = json!(reason);
                            }
                        }
                        if let (Some(id), Some(dir)) = (row["id"].as_str(), row["dir"].as_str()) {
                            if let Ok(mut connections) = app.state::<Controller>().connections.lock() {
                                if let Some(existing) = connections.iter_mut().find(|c| c.id == id) {
                                    if let Some(name) = row["name"].as_str() { existing.name = Some(name.to_string()); }
                                }
                                if !connections.iter().any(|c| c.id == id) { connections.push(Connection { id: id.into(), distro: distro.clone(), dir: dir.into(), enabled: true, name: row["name"].as_str().map(str::to_string) }); }
                            }
                        }
                        projects.push(row.clone());
                    }
                }
            }
        }
    }
    let saved = app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.clone();
    for c in saved.iter().filter(|c| !running.contains(&c.distro)) {
        projects.push(json!({ "id": c.id, "name": c.name, "dir": c.dir, "distro": c.distro, "running": false, "holder": null,
            "lastPoll": null, "logFile": "", "runtimes": [], "wslStopped": true }));
    }
    for row in projects.iter_mut() {
        let id = row["id"].as_str().map(str::to_string);
        if let Some(c) = id.and_then(|id| saved.iter().find(|c| c.id == id)) {
            row["enabled"] = json!(c.enabled);
            if !c.enabled { row.as_object_mut().map(|o| o.remove("stopReason")); }
        }
    }
    let _ = save_connections(&app);
    let setup = setup::snapshot(&running, &cli, refresh_auth == Some(true));
    Ok(json!({"schema":1, "version":version, "projects":projects, "runtimes":runtimes, "cli":cli, "setup":setup}))
}
