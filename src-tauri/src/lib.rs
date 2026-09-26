use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, fs, io::{BufRead, BufReader}, path::PathBuf, process::{Command, Stdio}, sync::{atomic::{AtomicBool, Ordering}, Arc, Mutex}, thread, time::{Duration, Instant}};
use tauri::{LogicalSize, PhysicalPosition, menu::{CheckMenuItem, Menu, MenuItem}, tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent}, AppHandle, Emitter, Manager};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_notification::NotificationExt;
use tauri_plugin_opener::OpenerExt;
use tauri_plugin_updater::UpdaterExt;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Connection { id: String, distro: String, dir: String }

#[derive(Default)]
struct Controller { jobs: Mutex<HashMap<String, Arc<AtomicBool>>>, connections: Mutex<Vec<Connection>>, tray_anchor: Mutex<Option<TrayAnchor>>, tray_hidden_at: Mutex<Option<Instant>>, pending_update: Mutex<Option<(tauri_plugin_updater::Update, Vec<u8>)>>, failures: Mutex<HashMap<String, String>>, update_checked: Mutex<Option<Instant>> }

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

/// Every wsl.exe this app starts is a console program; from a GUI process each
/// one would flash its own console window unless told not to.
#[cfg_attr(not(windows), allow(unused_mut))]
fn quiet(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

fn wsl(distro: &str, dir: Option<&str>, args: &[&str]) -> Command {
    let mut cmd = quiet(Command::new("wsl.exe"));
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
    Ok(binary_path_for_home(&run_wsl(distro, None, &["sh", "-c", "printf %s \"$HOME\""])?))
}

fn binary_path_for_home(home: &str) -> String { format!("{home}/.flowviant/bin/flowviant") }

fn parse_wsl_distros(output: &[u8]) -> Vec<String> {
    let decoded = if output.starts_with(&[0xff, 0xfe]) || (output.len() >= 2 && output[1] == 0) {
        String::from_utf16_lossy(&output.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>())
    } else { String::from_utf8_lossy(output).to_string() };
    decoded.lines().map(|line| line.trim_matches(['\u{feff}', '\0', '\r', ' ']).to_string())
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))).collect()
}

fn wsl_distros() -> Vec<String> {
    let Ok(output) = quiet(Command::new("wsl.exe")).args(["--list", "--quiet"]).output() else { return Vec::new() };
    if !output.status.success() { return Vec::new(); }
    // Docker Desktop's distros are not places anyone keeps a checkout, and
    // probing them on every refresh only costs time.
    parse_wsl_distros(&output.stdout).into_iter().filter(|name| !name.starts_with("docker-desktop")).collect()
}

fn merge_cleanup_distros(saved: &[Connection], discovered: Vec<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    saved.iter().map(|row| row.distro.clone()).chain(discovered)
        .filter(|distro| seen.insert(distro.clone())).collect()
}

// The uninstaller must finish even when WSL is stuck starting or a distro hangs.
fn cleanup_output(command: Command, deadline: Instant, capture: bool) -> Result<Vec<u8>, String> {
    cleanup_output_within(command, deadline, capture, Duration::from_secs(10))
}

// Probes get 10 s; the uninstall itself (stopping daemons, `npm uninstall -g`)
// gets the rest of the budget, because killing wsl.exe does not stop the Linux
// side and would leave the work half done.
fn cleanup_output_within(mut command: Command, deadline: Instant, capture: bool, cap: Duration) -> Result<Vec<u8>, String> {
    let call_deadline = deadline.min(Instant::now() + cap);
    if Instant::now() >= call_deadline { return Err("WSL timed out".into()); }
    command.stdout(if capture { Stdio::piped() } else { Stdio::null() }).stderr(Stdio::null());
    let mut child = command.spawn().map_err(|e| e.to_string())?;
    loop {
        let status = match child.try_wait() {
            Ok(status) => status,
            Err(error) => { let _ = child.kill(); let _ = child.wait(); return Err(error.to_string()); }
        };
        if let Some(status) = status {
            let output = child.wait_with_output().map_err(|e| e.to_string())?;
            return if status.success() { Ok(output.stdout) } else { Err("WSL command failed".into()) };
        }
        if Instant::now() >= call_deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("WSL timed out".into());
        }
        thread::sleep(Duration::from_millis(25).min(call_deadline.saturating_duration_since(Instant::now())));
    }
}

fn cleanup_wsl(distro: &str, args: &[&str], deadline: Instant, capture: bool) -> Result<Vec<u8>, String> {
    cleanup_output(wsl(distro, None, args), deadline, capture)
}

fn cleanup_binary_path(distro: &str, deadline: Instant) -> Result<String, String> {
    let home = cleanup_wsl(distro, &["sh", "-c", "printf %s \"$HOME\""], deadline, true)?;
    Ok(binary_path_for_home(String::from_utf8_lossy(&home).trim()))
}

pub fn uninstall_cleanup() {
    let deadline = Instant::now() + Duration::from_secs(60);
    // Tauri's Windows app_config_dir is %APPDATA%/<identifier>.
    let saved = std::env::var_os("APPDATA")
        .and_then(|base| fs::read(PathBuf::from(base).join("com.flowviant.desktop").join("connections.json")).ok())
        .and_then(|data| serde_json::from_slice::<Vec<Connection>>(&data).ok())
        .unwrap_or_default();
    let mut list = quiet(Command::new("wsl.exe"));
    list.args(["--list", "--quiet"]);
    let discovered = cleanup_output(list, deadline, true).map(|output| parse_wsl_distros(&output)).unwrap_or_default()
        .into_iter().filter(|distro| cleanup_wsl(distro, &["sh", "-c", "test -f \"$HOME/.flowviant/bin/flowviant\""], deadline, false).is_ok())
        .collect();
    for distro in merge_cleanup_distros(&saved, discovered) {
        if Instant::now() >= deadline { break; }
        let binary = cleanup_binary_path(&distro, deadline).ok();
        let present = binary.as_deref().is_some_and(|path| cleanup_wsl(&distro, &["test", "-f", path], deadline, false).is_ok());
        if let (true, Some(binary)) = (present, binary) {
            let _ = cleanup_output_within(wsl(&distro, None, &[&binary, "uninstall", "--yes", "--json"]), deadline, false, Duration::from_secs(60));
        } else {
            // Never `uninstall` through an unknown copy: a version without the
            // command can read it as "start the daemon". Every version knows `stop`.
            let _ = cleanup_wsl(&distro, &["sh", "-c", "command -v flowviant >/dev/null 2>&1 && flowviant stop"], deadline, false);
        }
    }
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
    let _ = run_wsl(distro, None, &[&target, "uninstall", "--others", "--yes", "--json"]);
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
    loop {
        if cancel.load(Ordering::SeqCst) { break; }
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
        let stderr = child.stderr.take().map(|err| thread::spawn(move || {
            let mut tail: Vec<String> = Vec::new();
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let line = readable(&line);
                if line.is_empty() { continue; }
                tail.push(line);
                if tail.len() > 5 { tail.remove(0); }
            }
            tail
        }));
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
        if cancel.load(Ordering::SeqCst) || displaced { break; }
        if clean_stop {
            // The server ended this box's part (removed from the project, its
            // credential revoked): final. Anything else that stopped it cleanly
            // was a SIGNAL — `flowviant stop`, or a `flowviant` started in a
            // terminal in the same repo, which takes over. It used to be final
            // too, so when that terminal closed the project sat offline with
            // the app open (Death Note - Roblox, 2026-09-25). Now: wait until
            // nothing else is serving it here, then serve it again.
            if matches!(stop_reason.as_deref(), Some("removed") | Some("credential-revoked")) { break; }
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

fn clear_failure(app: &AppHandle, id: &str) {
    if let Ok(mut failures) = app.state::<Controller>().failures.lock() { failures.remove(id); }
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

#[tauri::command(async)]
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
                        if row["running"] != json!(true) {
                            if let Some(reason) = row["id"].as_str().and_then(|id| app.state::<Controller>().failures.lock().ok().and_then(|f| f.get(id).cloned())) {
                                row["stopReason"] = json!(reason);
                            }
                        }
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

/// A device code as `flowviant login` prints it (`MJBG-JCX75H`).
fn valid_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 16 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The web page that takes the code, asks which project, and approves it.
fn connect_url(code: &str) -> String { format!("https://app.flowviant.com/connect?code={code}") }

/// One stderr line as a person can read it: wsl.exe's own errors arrive as
/// UTF-16 (NULs between the letters) and the daemon may colour its output.
fn readable(line: &str) -> String {
    let mut out = String::new();
    let mut chars = line.chars().filter(|c| *c != '\0').peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') { chars.next(); while let Some(n) = chars.next() { if n.is_ascii_alphabetic() { break; } } }
            continue;
        }
        out.push(c);
    }
    out.trim().to_string()
}

#[tauri::command(async)]
fn open_connect_page(app: AppHandle, code: String) -> Result<(), String> {
    if !valid_code(&code) { return Err("That code isn't one Flowviant printed".into()); }
    app.opener().open_url(connect_url(&code), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command(async)]
fn connect(app: AppHandle, folder: String) -> Result<(), String> {
    let (distro, dir) = parse_wsl_folder(&folder)?;
    ensure_binary(&app, &distro)?;
    thread::spawn(move || {
        let fail = |message: String| { let _ = app.emit("login-event", json!({"event":"error","message":message})); };
        let binary = match binary_path(&distro) { Ok(path) => path, Err(e) => { fail(e); return; } };
        let mut child = match wsl(&distro, Some(&dir), &[&binary, "login", "--json", "--dir", &dir, "--no-start"])
            .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
            Ok(child) => child, Err(e) => { fail(e.to_string()); return; }
        };
        // Drained on its own thread so a chatty stderr can never stall stdout;
        // its last lines are the reason when the login ends without one.
        let stderr = child.stderr.take().map(|err| thread::spawn(move || {
            let mut tail: Vec<String> = Vec::new();
            for line in BufReader::new(err).lines().map_while(Result::ok) {
                let line = readable(&line);
                if line.is_empty() { continue; }
                tail.push(line);
                if tail.len() > 5 { tail.remove(0); }
            }
            tail
        }));
        let mut completed = false;
        if let Some(stdout) = child.stdout.take() {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if let Ok(event) = serde_json::from_str::<Value>(&readable(&line)) {
                    if event["event"] == "open_url" {
                        match event["code"].as_str().filter(|code| valid_code(code)) {
                            Some(code) => { let _ = app.opener().open_url(connect_url(code), None::<&str>); }
                            None => if let Some(url) = event["url"].as_str() {
                                if url == "https://app.flowviant.com" || url.starts_with("https://app.flowviant.com/") { let _ = app.opener().open_url(url, None::<&str>); }
                            },
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
                            if let Err(e) = start_one(&app, row) { fail(format!("Connected, but the daemon did not start: {e}")); }
                        }
                    }
                    let _ = app.emit("login-event", event);
                }
            }
        }
        let status = child.wait();
        let tail = stderr.and_then(|handle| handle.join().ok()).unwrap_or_default();
        if !completed {
            let reason = tail.last().cloned()
                .or_else(|| status.ok().and_then(|s| s.code()).filter(|code| *code != 0).map(|code| format!("flowviant exited with code {code}")));
            fail(match reason { Some(reason) => format!("Connection was not completed: {reason}"), None => "Connection was not completed".into() });
        }
        let _ = app.emit("status-changed", ());
    });
    Ok(())
}

#[tauri::command(async)]
fn project_action(app: AppHandle, id: String, action: String) -> Result<(), String> {
    let row = connection(&app, &id)?;
    let binary = binary_path(&row.distro)?;
    if matches!(action.as_str(), "stop" | "restart" | "disconnect" | "forget") {
        if let Some(cancel) = app.state::<Controller>().jobs.lock().map_err(|e| e.to_string())?.remove(&id) { cancel.store(true, Ordering::SeqCst); }
        run_wsl(&row.distro, None, &[&binary, "stop", "--project", &id])?;
    }
    match action.as_str() {
        "start" | "restart" => {
            clear_failure(&app, &id);
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

fn valid_route_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 64 && value.bytes().all(|byte| byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'-')
}

#[tauri::command(async)]
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
    // To the popover only: the main window renders the same component, and a
    // broadcast "hidden" blanked it until the popover next opened.
    let _ = app.emit_to("tray", "tray-visible", false);
}

#[tauri::command]
fn hide_tray(app: AppHandle) { hide_tray_window(&app); }

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") { let _ = window.unminimize(); let _ = window.show(); let _ = window.set_focus(); }
    let _ = app.emit("main-visible", true);
}

/// `connect: true` is the popover's Connect a project: the folder picker is a
/// dialog, and a dialog would blur (and so close) the popover that asked for it,
/// so the window runs the flow.
#[tauri::command]
fn show_main(app: AppHandle, connect: Option<bool>) {
    hide_tray_window(&app);
    show_main_window(&app);
    if connect == Some(true) { let _ = app.emit_to("main", "begin-connect", ()); }
}

/// Launched by "Start at login" (the autostart entry passes this flag), the app
/// starts in the tray. Launched by a person, it opens its window.
const AUTOSTART_ARG: &str = "--autostart";

/// The same uninstaller Settings › Apps runs (NSIS writes it beside the exe).
/// It cleans WSL through the pre-uninstall hook; this app quits so the
/// uninstaller finds nothing running.
fn launch_uninstaller() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let uninstaller = exe.parent().ok_or("no install folder")?.join("uninstall.exe");
    if !uninstaller.is_file() { return Err("uninstaller not found".into()); }
    Command::new(uninstaller).spawn().map(|_| ()).map_err(|e| e.to_string())
}

/// Stop every daemon this app started, the way `flowviant stop` does.
fn stop_daemons(app: &AppHandle) {
    let rows = app.state::<Controller>().connections.lock().map(|rows| rows.clone()).unwrap_or_default();
    if let Ok(mut jobs) = app.state::<Controller>().jobs.lock() {
        for (_, cancel) in jobs.drain() { cancel.store(true, Ordering::SeqCst); }
    }
    for row in rows {
        if let Ok(binary) = binary_path(&row.distro) { let _ = run_wsl(&row.distro, None, &[&binary, "stop", "--project", &row.id]); }
    }
}

fn quit(app: &AppHandle) {
    stop_daemons(app);
    app.exit(0);
}

// ── Updating the app itself ──────────────────────────────────────────────────
//
// The app checks its manifest (`/dl/desktop/latest.json`, signed with the key in
// tauri.conf.json) a minute after it starts and every six hours, and downloads a
// newer installer in the background. It installs ON ITS OWN only when nothing is
// working on this computer: installing stops the daemons, and a turn cut off
// mid-way is lost work. "Nothing working" is measured per project this computer
// SERVES (an inactive project's agents run elsewhere); anything unmeasured counts
// as busy. The card also offers "Restart now" (a person's press, confirmed).
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
    *app.state::<Controller>().pending_update.lock().map_err(|e| e.to_string())? = Some((update, bytes));
    let _ = app.emit("update-ready", json!({ "version": version }));
    Ok(())
}

/// True only when no project whose daemon RUNS HERE and serves has a working
/// agent. A project not running on this computer has nothing here to cut off.
fn machine_idle(app: &AppHandle) -> bool {
    let rows = app.state::<Controller>().connections.lock().map(|rows| rows.clone()).unwrap_or_default();
    let distros: HashSet<String> = rows.iter().map(|row| row.distro.clone()).collect();
    for distro in distros {
        let Ok(binary) = binary_path(&distro) else { return false };
        let Ok(raw) = run_wsl(&distro, None, &[&binary, "status", "--json", "--remote"]) else { return false };
        let Ok(value) = serde_json::from_str::<Value>(&raw) else { return false };
        for project in value["projects"].as_array().into_iter().flatten() {
            if !rows.iter().any(|row| project["id"] == json!(row.id)) { continue; }
            if project["running"] != json!(true) { continue; }
            match project["remote"]["role"].as_str() {
                Some("inactive") => continue,
                Some("serving") => (),
                _ => return false,
            }
            let Some(agents) = project["remote"]["agents"].as_array() else { return false };
            if agents.iter().any(|agent| agent["status"] == "working" && agent["parked"] != json!(true)) { return false; }
        }
    }
    true
}

fn install_pending(app: &AppHandle) -> Result<(), String> {
    let staged = app.state::<Controller>().pending_update.lock().map_err(|e| e.to_string())?.take();
    let Some((update, bytes)) = staged else { return Err("No update is ready".into()) };
    stop_daemons(app);
    // On Windows this starts the installer and exits the process.
    update.install(bytes).map_err(|e| e.to_string())?;
    app.restart();
}

fn update_loop(app: AppHandle) {
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
async fn check_update(app: AppHandle, force: Option<bool>) -> Result<Option<String>, String> {
    if cfg!(debug_assertions) { return Ok(None); }
    check_now(&app, if force == Some(true) { Duration::ZERO } else { UPDATE_CHECK_ON_OPEN }).await
}

#[tauri::command]
fn app_version(app: AppHandle) -> String { app.package_info().version.to_string() }

#[tauri::command]
fn pending_update(app: AppHandle) -> Option<String> {
    app.state::<Controller>().pending_update.lock().ok().and_then(|p| p.as_ref().map(|(update, _)| update.version.clone()))
}

/// The card's "Restart now": a person chose to update while work may be running.
#[tauri::command(async)]
fn install_update(app: AppHandle) -> Result<(), String> { install_pending(&app) }

#[tauri::command(async)]
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
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)))
        .plugin(tauri_plugin_autostart::init(tauri_plugin_autostart::MacosLauncher::LaunchAgent, Some(vec![AUTOSTART_ARG])))
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(tauri::generate_handler![status, connect, open_connect_page, project_action, open_project, set_tray_state, fit_tray, hide_tray, show_main, quit_app, pending_update, install_update, check_update, app_version])
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
                } else if autostart.is_enabled().unwrap_or(false) {
                    // Rewrite an entry 0.0.3 and older registered without the flag,
                    // or every sign-in would open the window.
                    let _ = autostart.enable();
                }
            }
            let open = MenuItem::with_id(app, "open", "Open Flowviant Machines", true, None::<&str>)?;
            let login = CheckMenuItem::with_id(app, "login", "Start at login", true, autostart.is_enabled().unwrap_or(false), None::<&str>)?;
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
            if !std::env::args().any(|arg| arg == AUTOSTART_ARG) { show_main_window(handle); }
            update_loop(handle.clone());
            let startup = handle.clone();
            thread::spawn(move || {
                // Every connected project is supervised. One already served here
                // (a terminal `flowviant`) is not taken over: its supervisor stands
                // by until it ends, then serves it (see `supervise`).
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

#[cfg(test)]
mod tests {
    use super::{connect_url, merge_cleanup_distros, parse_wsl_distros, readable, valid_code, Connection};

    #[test]
    fn stderr_lines_read_as_words() {
        assert_eq!(readable("\u{1b}[31merror:\u{1b}[0m no git repository "), "error: no git repository");
        assert_eq!(readable("W\0S\0L\0 \0e\0r\0r\0o\0r\0"), "WSL error");
    }

    #[test]
    fn only_a_printed_code_reaches_the_connect_page() {
        assert!(valid_code("MJBG-JCX75H"));
        assert_eq!(connect_url("MJBG-JCX75H"), "https://app.flowviant.com/connect?code=MJBG-JCX75H");
        assert!(!valid_code(""));
        assert!(!valid_code("MJBG&next=https://evil"));
        assert!(!valid_code("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
    }

    #[test]
    fn cleanup_deduplicates_saved_and_discovered_distros() {
        let saved = vec![
            Connection { id: "first".into(), distro: "Ubuntu".into(), dir: "/one".into() },
            Connection { id: "second".into(), distro: "Ubuntu".into(), dir: "/two".into() },
            Connection { id: "third".into(), distro: "Debian".into(), dir: "/three".into() },
        ];
        assert_eq!(merge_cleanup_distros(&saved, vec!["Debian".into(), "Fedora".into(), "Fedora".into()]),
            vec!["Ubuntu", "Debian", "Fedora"]);
    }

    #[test]
    fn wsl_distro_list_decodes_utf16_and_ignores_invalid_names() {
        let output: Vec<u8> = "\u{feff}Ubuntu\r\nBad Name\r\nDebian\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse_wsl_distros(&output), vec!["Ubuntu", "Debian"]);
    }
}
