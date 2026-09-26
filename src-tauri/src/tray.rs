//! THE TRAY: its icon, its menu, the popover's placement beside the icon,
//! and showing or hiding the two windows.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). Window geometry (the
//! popover anchored to the icon, clamped to the monitor's work area, refit to
//! its content's height) and the icon's state change for reasons of their
//! own, apart from what the daemons are doing.

use std::time::{Duration, Instant};
use tauri::{LogicalSize, PhysicalPosition, menu::{CheckMenuItem, Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;

use crate::uninstall::launch_uninstaller;
use crate::{quit, Controller};

#[derive(Clone, Copy)]
pub(crate) struct TrayAnchor { right: f64, bottom: f64, left: f64, top: f64, work_right: f64, work_bottom: f64, scale: f64 }

pub(crate) fn tray_icon(app: &AppHandle, state: &str) {
    let bytes: &[u8] = match state {
        "attention" => include_bytes!("../icons/tray-attention.png"),
        "serving" => include_bytes!("../icons/tray-serving.png"),
        _ => include_bytes!("../icons/tray-idle.png"),
    };
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(icon) = tauri::image::Image::from_bytes(bytes) { let _ = tray.set_icon(Some(icon)); }
    }
}

#[tauri::command]
pub fn set_tray_state(app: AppHandle, state: String) {
    if matches!(state.as_str(), "serving" | "idle" | "attention") { tray_icon(&app, &state); }
}

fn hide_tray_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("tray") {
        if window.is_visible().unwrap_or(false) { if let Ok(mut at) = app.state::<Controller>().tray_hidden_at.lock() { *at = Some(Instant::now()); } }
        let _ = window.hide();
    }
    // To the popover only: the main window renders the same component, and a
    // broadcast "hidden" blanked it until the popover next opened.
    let _ = app.emit_to("tray", "tray-visible", false);
}

#[tauri::command]
pub fn hide_tray(app: AppHandle) { hide_tray_window(&app); }

pub(crate) fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") { let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus(); }
    let _ = app.emit("main-visible", true);
}

/// `connect: true` is the popover's Connect a project: the folder picker is a
/// dialog, and a dialog would blur (and so close) the popover that asked for it,
/// so the window runs the flow.
#[tauri::command]
pub fn show_main(app: AppHandle, connect: Option<bool>) {
    hide_tray_window(&app);
    show_main_window(&app);
    if connect == Some(true) { let _ = app.emit_to("main", "begin-connect", ()); }
}

#[tauri::command]
pub fn fit_tray(app: AppHandle, height: f64) -> Result<(), String> {
    if !height.is_finite() || height <= 0.0 { return Err("Invalid tray height".into()); }
    let anchor = app.state::<Controller>().tray_anchor.lock().map_err(|e| e.to_string())?.ok_or("Tray is not placed")?;
    let window = app.get_webview_window("tray").ok_or("Tray window is missing")?;
    let work_height = anchor.work_bottom - anchor.top;
    let logical_height = height.min(work_height / anchor.scale * 0.8);
    window.set_size(LogicalSize::new(352.0, logical_height)).map_err(|e| e.to_string())?;
    let x = (anchor.right - 352.0 * anchor.scale).clamp(anchor.left, (anchor.work_right - 352.0 * anchor.scale).max(anchor.left));
    let y = (anchor.bottom - logical_height * anchor.scale).clamp(anchor.top, (anchor.work_bottom - logical_height * anchor.scale).max(anchor.top));
    window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32)).map_err(|e| e.to_string())
}

fn place_tray(app: &AppHandle, rect: tauri::Rect) {
    let Some(window) = app.get_webview_window("tray") else { return };
    let position = rect.position.to_physical::<f64>(1.0);
    let size = rect.size.to_physical::<f64>(1.0);
    let center_x = position.x + size.width / 2.0;
    let center_y = position.y + size.height / 2.0;
    let Ok(Some(monitor)) = window.monitor_from_point(center_x, center_y) else { return };
    let work = monitor.work_area();
    let left = f64::from(work.position.x);
    let top = f64::from(work.position.y);
    let work_right = left + f64::from(work.size.width);
    let work_bottom = top + f64::from(work.size.height);
    let scale = monitor.scale_factor();
    let icon_right = position.x + size.width;
    let icon_bottom = position.y + size.height;
    let right = if icon_right <= left { icon_right + 352.0 * scale } else if position.x >= work_right { position.x } else { icon_right };
    let height = window.inner_size().map(|size| f64::from(size.height)).unwrap_or(480.0 * scale);
    let bottom = if icon_bottom <= top { top + height } else { position.y };
    let x = (right - 352.0 * scale).clamp(left, (work_right - 352.0 * scale).max(left));
    let y = (bottom - height).clamp(top, (work_bottom - height).max(top));
    let anchor = TrayAnchor { right: x + 352.0 * scale, bottom: y + height, left, top, work_right, work_bottom, scale };
    if let Ok(mut saved) = app.state::<Controller>().tray_anchor.lock() { *saved = Some(anchor); }
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
}

/// The tray icon and its menu, and the two windows' close/blur behaviour:
/// closing the window hides it (the app lives in the tray), and the popover
/// hides when it loses focus.
pub(crate) fn install(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "Open Flowviant Machines", true, None::<&str>)?;
    let login = CheckMenuItem::with_id(app, "login", "Start at login", true, app.autolaunch().is_enabled().unwrap_or(false), None::<&str>)?;
    let uninstall_item = MenuItem::with_id(app, "uninstall", "Uninstall Flowviant…", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &login, &uninstall_item, &quit_item])?;
    TrayIconBuilder::with_id("main").icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray-idle.png"))?)
        .menu(&menu).show_menu_on_left_click(false)
        .on_menu_event(move |app, event| match event.id().as_ref() {
            "open" => show_main_window(app),
            "login" => { let auto = app.autolaunch(); if auto.is_enabled().unwrap_or(false) { let _ = auto.disable(); } else { let _ = auto.enable(); } let _ = login.set_checked(auto.is_enabled().unwrap_or(false)); },
            "quit" => quit(app),
            "uninstall" => { if launch_uninstaller().is_ok() { quit(app); } },
            _ => (),
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, rect, .. } = event {
                if let Some(window) = tray.app_handle().get_webview_window("tray") {
                    // Clicking the icon while the popover is open blurs it first, so it is
                    // already hidden by the time this click lands: that click means "close".
                    let just_hidden = tray.app_handle().state::<Controller>().tray_hidden_at.lock().ok()
                        .and_then(|at| *at).is_some_and(|at| at.elapsed() < Duration::from_millis(300));
                    if window.is_visible().unwrap_or(false) { hide_tray_window(tray.app_handle()); }
                    else if !just_hidden { place_tray(tray.app_handle(), rect); let _ = window.show(); let _ = window.set_focus(); let _ = tray.app_handle().emit_to("tray", "tray-visible", true); }
                }
            }
        }).build(app)?;
    if let Some(window) = app.get_webview_window("main") {
        let window_for_close = window.clone();
        window.on_window_event(move |event| { if let tauri::WindowEvent::CloseRequested { api, .. } = event { api.prevent_close(); let _ = window_for_close.hide(); } });
    }
    if let Some(window) = app.get_webview_window("tray") {
        let tray_for_focus = window.clone();
        window.on_window_event(move |event| {
            if let tauri::WindowEvent::Focused(false) = event { hide_tray_window(tray_for_focus.app_handle()); }
        });
    }
    Ok(())
}
