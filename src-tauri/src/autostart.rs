//! START AT SIGN-IN: the entry `tauri-plugin-autostart` keeps for this app
//! in Windows (the current user's Run key, launching with `--autostart`), and
//! the two places a person turns it on or off: the window's switch and the
//! tray menu's check item, both worded "Start Flowviant when I sign in to
//! Windows". Launch writes no entry (`autostart_rules.rs` says why, and why
//! an existing one stays).
//!
//! THE ENTRY IS THE SETTING. The choice is kept where Windows keeps it, as
//! the tray menu's item always has, and read back from there whenever it is
//! shown, not copied into a file beside `connections.json`: Windows'
//! Settings › Apps › Startup can turn the same entry off, and a second copy
//! would disagree with it.

use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::autostart_rules::{at_launch, AtLaunch};
use crate::Controller;

/// The words on the window's switch and the tray menu's item.
pub(crate) const LABEL: &str = "Start Flowviant when I sign in to Windows";

/// Whether the app starts at sign-in; `None` when Windows could not say.
pub(crate) fn registered(app: &AppHandle) -> Option<bool> { app.autolaunch().is_enabled().ok() }

/// Launch: keep an entry that is there current, and never make one.
pub(crate) fn on_launch(app: &AppHandle) {
    if at_launch(registered(app)) == AtLaunch::Rewrite {
        // Enabling again rewrites the entry with `--autostart`, which 0.0.3
        // and older left out (every sign-in opened the window).
        let _ = app.autolaunch().enable();
    }
}

/// A person's press: add or remove the entry, then show both places what
/// Windows now says. An error is reported only when Windows did not end up
/// where the press asked.
pub(crate) fn set(app: &AppHandle, on: bool) -> Result<Option<bool>, String> {
    let manager = app.autolaunch();
    let result = if on { manager.enable() } else { manager.disable() };
    let now = registered(app);
    if let Some(item) = app.state::<Controller>().login_item.lock().ok().and_then(|item| item.clone()) { let _ = item.set_checked(now == Some(true)); }
    let _ = app.emit("start-at-login", now);
    match result {
        Err(error) if now != Some(on) => Err(format!("Windows did not change the sign-in entry: {error}")),
        _ => Ok(now),
    }
}

/// The window's switch reads this; `null` (unknown) draws no switch.
#[tauri::command]
pub fn start_at_login(app: AppHandle) -> Option<bool> { registered(&app) }

#[tauri::command(async)]
pub fn set_start_at_login(app: AppHandle, on: bool) -> Result<Option<bool>, String> { set(&app, on) }
