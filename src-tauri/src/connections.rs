//! THE PROJECTS THIS APP SERVES, and where the list is kept.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). A connection is one
//! project bound to one folder in one WSL distro; the list lives in
//! `connections.json` under the app's config dir and in `Controller`. Every
//! other module reads rows through here; the uninstaller reads the same file
//! by its Windows path because it runs with no Tauri app (`uninstall.rs`).

use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};
use tauri::{AppHandle, Manager};

use crate::Controller;

fn on() -> bool { true }

/// One project this app serves. `enabled` is the card's switch (absent in
/// connections saved before 0.0.14: on). `name` is remembered so a project in a
/// distro that is not running can still be listed without booting it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub(crate) struct Connection { pub(crate) id: String, pub(crate) distro: String, pub(crate) dir: String, #[serde(default = "on")] pub(crate) enabled: bool, #[serde(default)] pub(crate) name: Option<String> }

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("connections.json"))
}

pub(crate) fn save_connections(app: &AppHandle) -> Result<(), String> {
    let path = config_path(app)?;
    let rows = app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.clone();
    fs::write(path, serde_json::to_vec(&rows).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

pub(crate) fn connection(app: &AppHandle, id: &str) -> Result<Connection, String> {
    app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.iter().find(|c| c.id == id).cloned().ok_or_else(|| "Project is not connected on this box".into())
}

/// The saved connections, read once at launch (a missing or unreadable file
/// leaves the list empty).
pub(crate) fn load_connections(handle: &AppHandle) {
    if let Ok(path) = config_path(handle) {
        if let Ok(data) = fs::read(path) {
            if let Ok(rows) = serde_json::from_slice::<Vec<Connection>>(&data) { *handle.state::<Controller>().connections.lock().unwrap() = rows; }
        }
    }
}
