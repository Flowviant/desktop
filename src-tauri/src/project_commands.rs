//! A PERSON'S PRESSES ON ONE PROJECT'S ROW: Start, Stop, Restart,
//! Disconnect, Forget, the on/off switch, and the doors to the web app and
//! the daemon's log.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). Each press acts on one
//! saved connection through the daemon's own commands (`stop`, `machines
//! --remove|--forget`) and the supervisor; nothing here decides on its own.

use serde_json::Value;
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::connections::{connection, save_connections};
use crate::machine_log::log_line;
use crate::supervisor::{clear_failure, start_one};
use crate::wsl::{binary_path, distro_running, run_wsl};
use crate::Controller;

#[tauri::command(async)]
pub fn project_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let row = connection(&app, &id)?;
    let binary = binary_path(&row.distro)?;
    if matches!(action.as_str(), "stop" | "restart" | "disconnect" | "forget") {
        if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(&id) { cancel.store(true, Ordering::SeqCst); }
        run_wsl(&row.distro, None, &[&binary, "stop", "--project", &id])?;
    }
    match action.as_str() {
        "start" | "restart" => {
            clear_failure(&app, &id);
            log_line(&app, &format!("{}: {action} pressed", row.name.clone().unwrap_or_else(|| id.clone())));
            // A person's Start replaces a supervisor that is waiting out a
            // refusal or standing by, so the press acts now.
            if action == "start" {
                if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(&id) { cancel.store(true, Ordering::SeqCst); }
            }
            start_one(&app, row)?
        },
        "disconnect" | "forget" => {
            let verb = if action == "disconnect" { "--remove" } else { "--forget" };
            let raw = run_wsl(&row.distro, None, &[&binary, "machines", verb, &id, "--yes", "--json"])?;
            let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            if value["ok"] != true { return Err(value["error"].as_str().unwrap_or("Connection could not be removed").into()); }
            app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.retain(|c| c.id != id);
            save_connections(&app)?;
        }
        "stop" => (),
        _ => return Err("Unknown action".into()),
    }
    let _ = app.emit("status-changed", ());
    Ok(())
}

/// The card's per-project switch. Off stops the daemon and keeps it stopped —
/// no restart, not started at launch; on starts it and keeps it running.
#[tauri::command(async)]
pub fn set_enabled(app: AppHandle, id: String, enabled: bool) -> Result<(), String> {
    let row = {
        let controller = app.state::<Controller>();
        let mut rows = controller.connections.lock().map_err(|e| e.to_string())?;
        let row = rows.iter_mut().find(|c| c.id == id).ok_or("Project is not connected on this box")?;
        row.enabled = enabled;
        row.clone()
    };
    save_connections(&app)?;
    let who = row.name.clone().unwrap_or_else(|| row.id.clone());
    log_line(&app, &format!("{who}: switched {}", if enabled { "on" } else { "off" }));
    if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(&id) { cancel.store(true, Ordering::SeqCst); }
    clear_failure(&app, &id);
    if enabled {
        start_one(&app, row)?;
    } else if distro_running(&row.distro) {
        // Stopped through the daemon's own clean path; a stopped distro has
        // nothing to stop and is left stopped.
        let binary = binary_path(&row.distro)?;
        let _ = run_wsl(&row.distro, None, &[&binary, "stop", "--project", &id]);
    }
    let _ = app.emit("status-changed", ());
    Ok(())
}

fn valid_route_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[tauri::command(async)]
pub fn open_project(app: AppHandle, id: String, page: String, agent: Option<String>) -> Result<(), String> {
    if !valid_route_id(&id) || agent.as_deref().is_some_and(|value| !valid_route_id(value)) { return Err("Invalid project or agent ID".into()); }
    let row = connection(&app, &id)?;
    match page.as_str() {
        "board" | "machines" => {
            let suffix = if page == "board" { "board" } else { "machines" };
            app.opener().open_url(format!("https://app.flowviant.com/projects/{id}/{suffix}"), None::<&str>).map_err(|e| e.to_string())
        }
        "agent" => {
            let agent = agent.ok_or("Agent ID is required")?;
            app.opener().open_url(format!("https://app.flowviant.com/projects/{id}/v2/agents/{agent}"), None::<&str>).map_err(|e| e.to_string())
        }
        "logs" => {
            let binary = binary_path(&row.distro)?;
            let raw = run_wsl(&row.distro, None, &[&binary, "status", "--json"])?;
            let value: Value = serde_json::from_str(&raw).map_err(|e| e.to_string())?;
            let log = value["projects"].as_array().and_then(|rows| rows.iter().find(|r| r["id"] == id)).and_then(|r| r["logFile"].as_str()).ok_or("No log file is available")?;
            let windows_path = run_wsl(&row.distro, None, &["wslpath", "-w", log])?;
            app.opener().open_path(windows_path, None::<&str>).map_err(|e| e.to_string())
        }
        _ => Err("Unknown page".into()),
    }
}
