use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, fs, io::{BufRead, BufReader}, path::PathBuf, process::{Command, Stdio}, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, thread, time::{Duration, Instant}};
use tauri::{LogicalSize, PhysicalPosition, menu::{CheckMenuItem, Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Connection { id: String, distro: String, dir: String }

#[derive(Default)]
struct Controller { jobs: Mutex<HashMap<String, Arc<AtomicBool>>>, connections: Mutex<Vec<Connection>>, tray_anchor: Mutex<Option<TrayAnchor>>, tray_hidden_at: Mutex<Option<Instant>> }

#[derive(Clone, Copy)]
struct TrayAnchor { right: f64, bottom: f64, left: f64, top: f64, work_right: f64, work_bottom: f64, scale: f64 }

fn config_path(app: &AppHandle) -> Result<PathBuf, String> {
    let dir = app.path().app_config_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    Ok(dir.join("connections.json"))
}

fn save_connections(app: &AppHandle) -> Result<(), String> {
    let path = config_path(app)?;
    let rows = app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.clone();
    fs::write(path, serde_json::to_vec(&rows).map_err(|e| e.to_string())?).map_err(|e| e.to_string())
}

fn wsl(distro: &str, dir: Option<&str>, args: &[&str]) -> Command {
    let mut cmd = Command::new("wsl.exe");
    cmd.args(["-d", distro]);
    if let Some(dir) = dir { cmd.args(["--cd", dir]); }
    cmd.arg("--").args(args);
    cmd
}

fn run_wsl(distro: &str, dir: Option<&str>, args: &[&str]) -> Result<String, String> {
    let output = wsl(distro, dir, args).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn parse_wsl_folder(path: &str) -> Result<(String, String), String> {
    let normalized;
    let path = if path.to_ascii_lowercase().starts_with("\\\\?\\unc\\") {
        normalized = format!("\\\\{}", &path[8..]);
        normalized.as_str()
    } else { path };
    let parts: Vec<_> = path.split('\\').filter(|s| !s.is_empty()).collect();
    if !path.starts_with("\\\\") || parts.len() < 3 || !matches!(parts[0].to_ascii_lowercase().as_str(), "wsl$" | "wsl.localhost") {
        return Err("Windows folders come later — pick a folder inside WSL".into());
    }
    let distro = parts[1];
    if !distro.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) || parts[2..].iter().any(|s| *s == "." || *s == "..") {
        return Err("Pick a folder inside a WSL distro".into());
    }
    Ok((distro.into(), format!("/{}", parts[2..].join("/"))))
}

fn version(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').take(3).map(|s| s.parse().unwrap_or(0)).collect()
}

fn binary_path(distro: &str) -> Result<String, String> {
    Ok(format!("{}/.flowviant/bin/flowviant", run_wsl(distro, None, &["sh", "-c", "printf %s \"$HOME\""])?))
}

fn wsl_distros() -> Vec<String> {
    let Ok(output) = Command::new("wsl.exe").args(["--list", "--quiet"]).output() else { return Vec::new() };
    if !output.status.success() { return Vec::new(); }
    let decoded = if output.stdout.len() >= 2 && output.stdout[1] == 0 {
        String::from_utf16_lossy(&output.stdout.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>())
    } else { String::from_utf8_lossy(&output.stdout).to_string() };
    decoded.lines().map(|line| line.trim_matches(['\u{feff}', '\0', '\r', ' ']).to_string())
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))).collect()
}

fn ensure_binary(app: &AppHandle, distro: &str) -> Result<(), String> {
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
    Ok(())
}

fn tray_icon(app: &AppHandle, state: &str) {
    let bytes: &[u8] = match state {
        "attention" => include_bytes!("../icons/tray-attention.png"),
        "serving" => include_bytes!("../icons/tray-serving.png"),
        _ => include_bytes!("../icons/tray-idle.png"),
    };
    if let Some(tray) = app.tray_by_id("main") {
        if let Ok(icon) = tauri::image::Image::from_bytes(bytes) { let _ = tray.set_icon(Some(icon)); }
    }
}

fn announce(app: &AppHandle, event: &Value) {
    let _ = app.emit("machine-event", event);
    let kind = event.get("event").and_then(Value::as_str).unwrap_or("");
    if matches!(kind, "displaced" | "stopped-unexpectedly" | "limit-hit" | "update-applied") {
        let body = event.get("message").and_then(Value::as_str).map(str::to_string).unwrap_or_else(|| match kind {
            "displaced" => "This box stopped serving after another box took the machine".into(),
            "stopped-unexpectedly" => "The daemon stopped unexpectedly".into(),
            "update-applied" => "The daemon updated".into(),
            _ => "A CLI hit its limit".into(),
        });
        let _ = app.notification().builder().title("Flowviant machine").body(body).show();
        if kind != "update-applied" { tray_icon(app, "attention"); }
    } else if kind == "serving" { tray_icon(app, "serving"); }
}

fn supervise(app: AppHandle, connection: Connection, cancel: Arc<AtomicBool>) {
    let mut backoff = 2;
    loop {
        if cancel.load(Ordering::SeqCst) { break; }
        let binary = match binary_path(&connection.distro) { Ok(path) => path, Err(_) => break };
        let result = wsl(&connection.distro, Some(&connection.dir), &[&binary, "--project", &connection.id, "--dir", &connection.dir, "--json-events"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn();
        let Ok(mut child) = result else {
            announce(&app, &json!({"event":"stopped-unexpectedly", "projectId":connection.id}));
            break;
        };
        let mut displaced = false;
        let mut clean_stop = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(mut event) = serde_json::from_str::<Value>(&line) {
                    if event.get("event").and_then(Value::as_str) == Some("displaced") { displaced = true; }
                    if event.get("event").and_then(Value::as_str) == Some("stopped") { clean_stop = true; }
                    event["projectId"] = json!(connection.id);
                    announce(&app, &event);
                }
            }
        }
        let _ = child.wait();
        let _ = app.emit("status-changed", ());
        if cancel.load(Ordering::SeqCst) || displaced || clean_stop { break; }
        announce(&app, &json!({"event":"stopped-unexpectedly", "projectId":connection.id}));
        for _ in 0..backoff * 10 { if cancel.load(Ordering::SeqCst) { break; } thread::sleep(Duration::from_millis(100)); }
        backoff = (backoff * 2).min(60);
    }
    if let Ok(mut jobs) = app.state::<Controller>().jobs.lock() {
        if jobs.get(&connection.id).is_some_and(|current| Arc::ptr_eq(current, &cancel)) { jobs.remove(&connection.id); }
    }
}

fn start_one(app: &AppHandle, connection: Connection) -> Result<(), String> {
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

fn connection(app: &AppHandle, id: &str) -> Result<Connection, String> {
    app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.iter().find(|c| c.id == id).cloned().ok_or_else(|| "Project is not connected on this box".into())
}

#[tauri::command]
fn status(app: AppHandle, remote: Option<bool>) -> Result<Value, String> {
    let known = app.state::<Controller>().connections.lock().map_err(|e| e.to_string())?.clone();
    let mut distros: HashSet<_> = known.iter().map(|c| c.distro.clone()).collect();
    distros.extend(wsl_distros());
    let mut projects = Vec::new();
    let mut runtimes = Vec::new();
    let mut cli = Vec::new();
    let mut version = Value::Null;
    for distro in distros {
        let remote_raw = if remote == Some(true) {
            binary_path(&distro).ok().and_then(|binary| run_wsl(&distro, None, &[&binary, "status", "--json", "--remote"]).ok())
                .or_else(|| run_wsl(&distro, None, &["flowviant", "status", "--json", "--remote"]).ok())
                .filter(|raw| serde_json::from_str::<Value>(raw).is_ok())
        } else { None };
        let raw = remote_raw.or_else(|| binary_path(&distro).ok().and_then(|binary| run_wsl(&distro, None, &[&binary, "status", "--json"]).ok())
            .or_else(|| run_wsl(&distro, None, &["flowviant", "status", "--json"]).ok()));
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
                        if let (Some(id), Some(dir)) = (row["id"].as_str(), row["dir"].as_str()) {
                            if let Ok(mut connections) = app.state::<Controller>().connections.lock() {
                                if !connections.iter().any(|c| c.id == id) { connections.push(Connection { id: id.into(), distro: distro.clone(), dir: dir.into() }); }
                            }
                        }
                        projects.push(row.clone());
                    }
                }
            }
        }
    }
    let _ = save_connections(&app);
    Ok(json!({"schema":1, "version":version, "projects":projects, "runtimes":runtimes, "cli":cli}))
}

#[tauri::command]
fn connect(app: AppHandle, folder: String) -> Result<(), String> {
    let (distro, dir) = parse_wsl_folder(&folder)?;
    ensure_binary(&app, &distro)?;
    thread::spawn(move || {
        let binary = match binary_path(&distro) { Ok(path) => path, Err(e) => { let _ = app.emit("login-event", json!({"event":"error","message":e})); return; } };
        let mut child = match wsl(&distro, Some(&dir), &[&binary, "login", "--json", "--dir", &dir, "--no-start"])
            .stdout(Stdio::piped()).stderr(Stdio::null()).spawn() {
            Ok(child) => child, Err(e) => { let _ = app.emit("login-event", json!({"event":"error","message":e.to_string()})); return; }
        };
        let mut completed = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<Value>(&line) {
                    if event["event"] == "open_url" {
                        if let Some(url) = event["url"].as_str() {
                            if url == "https://app.flowviant.com" || url.starts_with("https://app.flowviant.com/") { let _ = app.opener().open_url(url, None::<&str>); }
                        }
                    }
                    if event["event"] == "error" { completed = true; }
                    if event["event"] == "bound" {
                        completed = true;
                        if let Some(id) = event["projectId"].as_str() {
                            let row = Connection { id: id.into(), distro: distro.clone(), dir: dir.clone() };
                            if let Ok(mut rows) = app.state::<Controller>().connections.lock() {
                                rows.retain(|existing| existing.id != id);
                                rows.push(row.clone());
                            }
                            let _ = save_connections(&app);
                            let _ = start_one(&app, row);
                        }
                    }
                    let _ = app.emit("login-event", event);
                }
            }
        }
        let _ = child.wait();
        if !completed { let _ = app.emit("login-event", json!({"event":"error", "message":"Connection was not completed"})); }
        let _ = app.emit("status-changed", ());
    });
    Ok(())
}

#[tauri::command]
fn project_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let row = connection(&app, &id)?;
    let binary = binary_path(&row.distro)?;
    if matches!(action.as_str(), "stop" | "restart" | "disconnect" | "forget") {
        if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(&id) { cancel.store(true, Ordering::SeqCst); }
        run_wsl(&row.distro, None, &[&binary, "stop", "--project", &id])?;
    }
    match action.as_str() {
        "start" | "restart" => start_one(&app, row)?,
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

fn valid_route_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[tauri::command]
fn open_project(app: AppHandle, id: String, page: String, agent: Option<String>) -> Result<(), String> {
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

#[tauri::command]
fn set_tray_state(app: AppHandle, state: String) {
    if matches!(state.as_str(), "serving" | "idle" | "attention") { tray_icon(&app, &state); }
}

fn hide_tray_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("tray") {
        if window.is_visible().unwrap_or(false) { if let Ok(mut at) = app.state::<Controller>().tray_hidden_at.lock() { *at = Some(Instant::now()); } }
        let _ = window.hide();
    }
    let _ = app.emit("tray-visible", false);
}

#[tauri::command]
fn hide_tray(app: AppHandle) { hide_tray_window(&app); }

#[tauri::command]
fn show_main(app: AppHandle) {
    hide_tray_window(&app);
    if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); }
}

fn quit(app: &AppHandle) {
    let rows = app.state::<Controller>().connections.lock().unwrap().clone();
    for row in rows {
        if let Ok(binary) = binary_path(&row.distro) { let _ = run_wsl(&row.distro, None, &[&binary, "stop", "--project", &row.id]); }
    }
    app.exit(0);
}

#[tauri::command]
fn quit_app(app: AppHandle) { quit(&app); }

#[tauri::command]
fn fit_tray(app: AppHandle, height: f64) -> Result<(), String> {
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

pub fn run() {
    tauri::Builder::default()
        .manage(Controller::default())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } }))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, None))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![status, connect, project_action, open_project, set_tray_state, fit_tray, hide_tray, show_main, quit_app])
        .setup(|app| {
            let handle = app.handle();
            if let Ok(path) = config_path(handle) {
                if let Ok(data) = fs::read(path) {
                    if let Ok(rows) = serde_json::from_slice::<Vec<Connection>>(&data) { *handle.state::<Controller>().connections.lock().unwrap() = rows; }
                }
            }
            let autostart = handle.autolaunch();
            if let Ok(marker) = handle.path().app_config_dir() {
                let marker = marker.join("autostart-initialized");
                if !marker.exists() {
                    let _ = autostart.enable();
                    let _ = fs::write(marker, b"1");
                }
            }
            let open = MenuItem::with_id(app, "open", "Open window", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "Start at login", true, autostart.is_enabled().unwrap_or(false), None::<&str>)?;
            let quit_item = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &login, &quit_item])?;
            TrayIconBuilder::with_id("main").icon(tauri::image::Image::from_bytes(include_bytes!("../icons/tray-idle.png"))?)
                .menu(&menu).show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id().as_ref() {
                    "open" => { if let Some(window) = app.get_webview_window("main") { let _ = window.show(); let _ = window.set_focus(); } },
                    "login" => { let auto = app.autolaunch(); if auto.is_enabled().unwrap_or(false) { let _ = auto.disable(); } else { let _ = auto.enable(); } let _ = login.set_checked(auto.is_enabled().unwrap_or(false)); },
                    "quit" => quit(app),
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
                            else if !just_hidden { place_tray(tray.app_handle(), rect); let _ = window.show(); let _ = window.set_focus(); let _ = tray.app_handle().emit("tray-visible", true); }
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
            let startup = handle.clone();
            thread::spawn(move || {
                let _ = status(startup.clone(), None);
                let rows = startup.state::<Controller>().connections.lock().unwrap().clone();
                for row in rows { let _ = start_one(&startup, row); }
                let _ = startup.emit("status-changed", ());
            });
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("Flowviant desktop failed");
}
