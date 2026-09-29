//! FLOWVIANT MACHINES, the Windows tray app: the composition point.
//!
//! This file registers the Tauri commands, holds the one shared `Controller`
//! state, and wires launch (saved connections, autostart, the tray, the update
//! loop, a supervisor per switched-on project) and quit. Each lifecycle has
//! its own module (split 2026-09-26, SOLID F161):
//!   * `autostart` (+ its pure rule `autostart_rules`) — starting at sign-in,
//!     only ever by a person's press;
//!   * `connections` — the projects this app serves and `connections.json`;
//!   * `wsl` — every `wsl.exe` this app starts and what it prints (std-only);
//!   * `bundled_daemon` — putting the shipped daemon in place in a distro;
//!   * `daemon_output` — a daemon's stderr as words (std-only);
//!   * `supervisor` — one project's daemon: start, relay, restart, stand by;
//!     its decisions are `supervisor_rules` (pure, std-only);
//!   * `status`, `connect`, `project_commands` — the card's reads and presses;
//!   * `self_update` (+ its pure rule `updates`) — updating the app itself;
//!   * `notifications` — which machine events reach the OS (pure);
//!   * `tray` — the icon, its menu, the popover's placement, the windows;
//!   * `machine_log` — the app's own machines.log;
//!   * `uninstall` — cleaning WSL on the way out.

use std::{collections::{HashMap, HashSet}, process::ChildStdin, sync::{atomic::AtomicBool, Arc, Mutex}, thread, time::Instant};
use tauri::{menu::CheckMenuItem, AppHandle, Emitter, Manager, Wry};

mod autostart;
mod autostart_rules;
mod bundled_daemon;
mod connect;
mod connections;
mod daemon_output;
mod machine_log;
mod notifications;
mod project_commands;
mod self_update;
mod setup;
mod setup_rules;
mod status;
mod supervisor;
mod supervisor_rules;
mod tray;
mod uninstall;
mod updates;
mod wsl;

pub use uninstall::uninstall_cleanup;

use connections::{load_connections, Connection};
use machine_log::log_line;
use supervisor::{start_one, stop_daemons};
use tray::{show_main_window, TrayAnchor};

#[derive(Default)]
struct Controller { jobs: Mutex<HashMap<String, Arc<AtomicBool>>>, connections: Mutex<Vec<Connection>>, tray_anchor: Mutex<Option<TrayAnchor>>, tray_hidden_at: Mutex<Option<Instant>>, pending_update: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>, failures: Mutex<HashMap<String, String>>, standing_by: Mutex<HashSet<String>>, update_checked: Mutex<Option<Instant>>, setup_login: Mutex<Option<ChildStdin>>, setup_login_busy: AtomicBool, login_item: Mutex<Option<CheckMenuItem<Wry>>> }

/// Launched at sign-in (the entry `autostart.rs` writes passes this flag), the
/// app starts in the tray. Launched by a person, it opens its window.
const AUTOSTART_ARG: &str = "--autostart";

fn quit(app: &AppHandle) {
    stop_daemons(app);
    app.exit(0);
}

#[tauri::command(async)]
fn quit_app(app: AppHandle) { quit(&app); }

pub fn run() {
    tauri::Builder::default()
        .manage(Controller::default())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![status::status, setup::setup_action, setup::setup_code, connect::connect, connect::open_connect_page, project_commands::project_action, project_commands::open_project, tray::set_tray_state, tray::fit_tray, tray::hide_tray, tray::show_main, quit_app, self_update::pending_update, self_update::install_update, self_update::check_update, self_update::app_version, project_commands::set_enabled, machine_log::open_logs, autostart::start_at_login, autostart::set_start_at_login])
        .setup(|app| {
            let handle = app.handle();
            load_connections(handle);
            // Start at sign-in is the person's to turn on (window switch, tray
            // menu); launch adds no entry and keeps one an older version added.
            autostart::on_launch(handle);
            tray::install(app)?;
            if !std::env::args().any(|arg| arg == AUTOSTART_ARG) { show_main_window(handle); }
            self_update::update_loop(handle.clone());
            let startup = handle.clone();
            thread::spawn(move || {
                // Every connected project is supervised. One already served here
                // (a terminal `flowviant`) is not taken over: its supervisor stands
                // by until it ends, then serves it (see `supervise`).
                let _ = status::status(startup.clone(), None, None);
                let rows = startup.state::<Controller>().connections.lock().unwrap().clone();
                log_line(&startup, &format!("Flowviant Machines {} started{}", startup.package_info().version,
                    if std::env::args().any(|arg| arg == AUTOSTART_ARG) { " at sign-in" } else { "" }));
                for row in rows.into_iter().filter(|row| row.enabled) { let _ = start_one(&startup, row); }
                let _ = startup.emit("status-changed", ());
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Flowviant desktop failed");
}
