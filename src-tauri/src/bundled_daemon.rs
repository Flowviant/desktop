//! PUTTING THE BUNDLED DAEMON IN PLACE in a distro before anything runs it.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). Both doors that start a
//! daemon — connecting a folder (`connect.rs`) and supervising a project
//! (`supervisor.rs`) — first make sure `~/.flowviant/bin/flowviant` in that
//! distro is at least the version this app ships, then let it remove older
//! copies itself.

use std::fs;
use tauri::{AppHandle, Manager};

use crate::wsl::{binary_path, run_wsl, version};

pub(crate) fn ensure_binary(app: &AppHandle, distro: &str) -> Result<(), String> {
    let source = app.path().resolve("resources/flowviant-linux-x64", tauri::path::BaseDirectory::Resource).map_err(|e| e.to_string())?;
    let version_path = app.path().resolve("resources/daemon-version.txt", tauri::path::BaseDirectory::Resource).map_err(|e| e.to_string())?;
    let bundled = fs::read_to_string(version_path).map_err(|e| e.to_string())?;
    let target = binary_path(distro)?;
    if let Ok(installed) = run_wsl(distro, None, &[&target, "--version"]) {
        if version(&installed) >= version(&bundled) { return Ok(()); }
    }
    let home = run_wsl(distro, None, &["sh", "-c", "printf %s \"$HOME\""])?;
    if !home.starts_with('/') || home.contains("..") { return Err("WSL home path is invalid".into()); }
    run_wsl(distro, None, &["mkdir", "-p", &format!("{home}/.flowviant/bin")])?;
    let unc = format!("\\\\wsl.localhost\\{distro}{}\\.flowviant\\bin\\flowviant.new", home.replace('/', "\\"));
    fs::copy(source, unc).map_err(|e| e.to_string())?;
    run_wsl(distro, None, &["mv", &format!("{home}/.flowviant/bin/flowviant.new"), &format!("{home}/.flowviant/bin/flowviant")])?;
    run_wsl(distro, None, &["chmod", "700", &format!("{home}/.flowviant/bin/flowviant")])?;
    let _ = run_wsl(distro, None, &[&target, "uninstall", "--others", "--yes", "--json"]);
    Ok(())
}
