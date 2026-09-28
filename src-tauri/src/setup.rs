//! Setup I/O for the WSL route. The pure offer/order policy is setup_rules.rs.
//! Polling only reads `wsl --status`/`--list` and already-running distros.
//! A person's explicit Open press may start a stopped distro.

use serde_json::{json, Value};
use std::{collections::{HashMap, HashSet}, io::{ErrorKind, Read, Write}, process::{ChildStdin, Command, Output, Stdio}, sync::{atomic::Ordering, Arc, Mutex, OnceLock}, thread, time::{Duration, Instant}};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::setup_rules::{classify_wsl, login_url, remeasure_after_press, wsl_steps, ClaudeState, Step, WslState};
use crate::wsl::{decode_wsl, parse_wsl_distros, quiet, wsl};
use crate::Controller;

fn bounded_output(mut command: Command, timeout: Duration) -> Result<Option<Output>, ErrorKind> {
    let mut child = command.stdout(Stdio::piped()).stderr(Stdio::piped()).spawn().map_err(|e| e.kind())?;
    let began = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let mut stdout = Vec::new(); let mut stderr = Vec::new();
                child.stdout.take().expect("piped stdout").read_to_end(&mut stdout).map_err(|e| e.kind())?;
                child.stderr.take().expect("piped stderr").read_to_end(&mut stderr).map_err(|e| e.kind())?;
                return Ok(Some(Output { status, stdout, stderr }));
            }
            Ok(None) if began.elapsed() < timeout => thread::sleep(Duration::from_millis(50)),
            _ => { let _ = child.kill(); let _ = child.wait(); return Ok(None); }
        }
    }
}

fn wsl_inventory() -> (WslState, Vec<String>) {
    let mut status_cmd = quiet(Command::new("wsl.exe"));
    status_cmd.arg("--status");
    let status_result = bounded_output(status_cmd, Duration::from_secs(3));
    let mut list_cmd = quiet(Command::new("wsl.exe"));
    list_cmd.args(["--list", "--quiet"]);
    let list_result = bounded_output(list_cmd, Duration::from_secs(3));
    let missing = matches!(&status_result, Err(ErrorKind::NotFound)) && matches!(&list_result, Err(ErrorKind::NotFound));
    let status = status_result.ok().flatten();
    let list = list_result.ok().flatten();
    let names = list.as_ref().map(|o| parse_wsl_distros(&o.stdout)).unwrap_or_default()
        .into_iter().filter(|name| !name.starts_with("docker-desktop")).collect::<Vec<_>>();
    let status_words = status.as_ref().map(|o| format!("{} {}", decode_wsl(&o.stdout), decode_wsl(&o.stderr))).unwrap_or_default();
    let list_words = list.as_ref().map(|o| format!("{} {}", decode_wsl(&o.stdout), decode_wsl(&o.stderr))).unwrap_or_default();
    let state = if missing { WslState::Missing } else { classify_wsl(status.as_ref().is_some_and(|o| o.status.success()), &status_words,
        list.as_ref().is_some_and(|o| o.status.success()), &list_words, &names) };
    (state, names)
}

fn state_word(state: WslState) -> &'static str {
    match state { WslState::Missing => "missing", WslState::NoDistro => "noDistro", WslState::Ready => "ready", WslState::Unknown => "unknown" }
}

fn step_word(step: Step) -> &'static str {
    match step { Step::InstallWsl => "installWsl", Step::OpenDistro => "openDistro", Step::InstallClaude => "installClaude", Step::SignInClaude => "signInClaude" }
}

fn auth_from_json(value: &Value) -> Value {
    let signed = value["loggedIn"].as_bool();
    if signed == Some(false) { return json!({"signedIn":false,"billing":null,"subscriptionType":null}); }
    if signed != Some(true) { return json!({"signedIn":null,"billing":null,"subscriptionType":null}); }
    let method = value["authMethod"].as_str().unwrap_or("").to_ascii_lowercase();
    let api = ["console", "api_key", "apikey", "bedrock", "vertex", "foundry"].iter().any(|part| method.contains(part));
    let sub = value["subscriptionType"].as_str().filter(|s| matches!(*s, "pro" | "max" | "team" | "enterprise"));
    let plan = !api && (method.contains("claude") || method.contains("subscription") || sub.is_some());
    json!({"signedIn":true,"billing":if api {Some("api")} else if plan {Some("subscription")} else {None},
        "subscriptionType":if plan {sub} else {None}})
}

fn direct_claude(distro: &str) -> Value {
    // The native installer puts Claude here; a noninteractive WSL shell does
    // not necessarily include it on PATH until the next login shell.
    let path = "export PATH=\"$HOME/.local/bin:$PATH\"; command -v claude >/dev/null 2>&1";
    let installed = bounded_output(wsl(distro, None, &["sh", "-lc", path]), Duration::from_secs(3)).ok().flatten();
    let Some(installed) = installed else { return json!({"installed":null,"signedIn":null,"billing":null,"subscriptionType":null}); };
    if !installed.status.success() { return json!({"installed":false,"signedIn":null,"billing":null,"subscriptionType":null}); }
    let result = bounded_output(wsl(distro, None, &["sh", "-lc", "export PATH=\"$HOME/.local/bin:$PATH\"; claude auth status --json"]), Duration::from_secs(4)).ok().flatten();
    let auth = result.filter(|out| out.status.success() || out.status.code() == Some(1))
        .and_then(|out| serde_json::from_slice::<Value>(&out.stdout).ok())
        .map(|v| auth_from_json(&v)).unwrap_or(json!({"signedIn":null,"billing":null,"subscriptionType":null}));
    let mut row = json!({"installed":true});
    if let (Some(dst), Some(src)) = (row.as_object_mut(), auth.as_object()) {
        for (key, value) in src { dst.insert(key.clone(), value.clone()); }
    }
    row
}

static DIRECT_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Value)>>> = OnceLock::new();

fn cached_claude(distro: &str, refresh: bool) -> Value {
    let cache = DIRECT_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if !refresh {
        if let Ok(rows) = cache.lock() {
            if let Some((at, value)) = rows.get(distro) {
                if at.elapsed() < Duration::from_secs(90) { return value.clone(); }
            }
        }
    }
    let value = direct_claude(distro);
    if let Ok(mut rows) = cache.lock() { rows.insert(distro.into(), (Instant::now(), value.clone())); }
    value
}

fn claude_for(distro: &str, daemon_cli: &[Value], refresh: bool) -> Value {
    if let Some(runtime) = daemon_cli.iter().find(|row| row["distro"].as_str() == Some(distro))
        .and_then(|row| row["runtimes"].as_array())
        .and_then(|rows| rows.iter().find(|row| row["id"].as_str() == Some("claude"))) {
        if runtime["installed"].as_bool() == Some(false) {
            return json!({"installed":false,"signedIn":null,"billing":null,"subscriptionType":null});
        }
        if runtime["installed"].as_bool() == Some(true) && runtime["signedIn"].as_bool().is_some() {
            return json!({"installed":true,"signedIn":runtime["signedIn"].clone(),
                "billing":runtime["billing"].clone(),"subscriptionType":runtime["subscriptionType"].clone()});
        }
    }
    cached_claude(distro, refresh)
}

pub(crate) fn snapshot(running: &HashSet<String>, daemon_cli: &[Value], refresh: bool) -> Value {
    let (measured_wsl, mut names) = wsl_inventory();
    for name in running { if !names.contains(name) { names.push(name.clone()); } }
    names.sort();
    let wsl_state = if !running.is_empty() { WslState::Ready } else { measured_wsl };
    let mut distros = Vec::new();
    for name in names {
        let active = running.contains(&name);
        let claude = if active { claude_for(&name, daemon_cli, refresh) } else { json!({"installed":null,"signedIn":null,"billing":null,"subscriptionType":null}) };
        let state = if claude["installed"].as_bool() == Some(false) { ClaudeState::Missing }
            else if claude["signedIn"].as_bool() == Some(false) { ClaudeState::SignedOut }
            else if claude["billing"].as_str() == Some("api") { ClaudeState::ApiBilling }
            else if claude["signedIn"].as_bool() == Some(true) { ClaudeState::Ready } else { ClaudeState::Unknown };
        let steps: Vec<_> = wsl_steps(wsl_state, active, state).into_iter().map(step_word).collect();
        distros.push(json!({"name":name,"running":active,"claude":claude,"steps":steps}));
    }
    let steps: Vec<_> = if matches!(wsl_state, WslState::Missing | WslState::NoDistro) {
        vec![step_word(Step::InstallWsl)]
    } else { Vec::new() };
    json!({"route":"wsl","wsl":state_word(wsl_state),"steps":steps,"distros":distros})
}

fn send(app: &AppHandle, phase: &str, action: &str, message: &str) {
    let _ = app.emit("setup-event", json!({"phase":phase,"action":action,"message":message}));
}

fn remeasure(app: &AppHandle, succeeded: bool) {
    if remeasure_after_press(succeeded) { let _ = app.emit("status-changed", ()); }
}

fn valid_distro(name: &str) -> bool {
    !name.is_empty() && name.len() <= 80 && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
}

fn person_distro(name: Option<String>) -> Result<String, String> {
    let name = name.ok_or("Choose a Linux distribution")?;
    if !valid_distro(&name) { return Err("Invalid Linux distribution".into()); }
    let (_, installed) = wsl_inventory();
    if !installed.contains(&name) { return Err("That Linux distribution is not installed".into()); }
    Ok(name)
}

fn install_wsl(app: AppHandle) {
    let (state, _) = wsl_inventory();
    if !matches!(state, WslState::Missing | WslState::NoDistro) {
        send(&app, "error", "installWsl", "WSL was not measured as missing"); remeasure(&app, false); return;
    }
    send(&app, "progress", "installWsl", "Windows will ask for permission to install Ubuntu.");
    let arg = if state == WslState::NoDistro { "--install -d Ubuntu" } else { "--install" };
    let command = format!("$p = Start-Process -FilePath wsl.exe -ArgumentList '{arg}' -Verb RunAs -Wait -PassThru; exit $p.ExitCode");
    let outcome = quiet(Command::new("powershell.exe")).args(["-NoProfile", "-NonInteractive", "-Command", &command]).output();
    let succeeded = outcome.as_ref().is_ok_and(|out| out.status.success());
    match outcome {
        Ok(out) if out.status.success() => send(&app, "restart", "installWsl", if state == WslState::NoDistro {
            "Open Ubuntu to make a Linux username and password. Restart Windows first if it asks."
        } else { "Restart Windows, then open Ubuntu to make a Linux username and password." }),
        Ok(out) => send(&app, "error", "installWsl", &format!("Windows did not finish installing WSL: {}", decode_wsl(&out.stderr).trim())),
        Err(error) => send(&app, "error", "installWsl", &format!("Windows did not start WSL setup: {error}")),
    }
    remeasure(&app, succeeded);
}

fn install_claude(app: AppHandle, distro: String) {
    // The press is the only write. `bash` is the vendor's installer shell; a
    // failed curl must fail the row rather than being masked by a successful bash.
    let mut child = match wsl(&distro, None, &["bash", "-lc", "set -o pipefail; curl -fsSL https://claude.ai/install.sh | bash"])
        .stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child, Err(error) => { send(&app, "error", "installClaude", &error.to_string()); remeasure(&app, false); return; }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let stdout_thread = stdout.map(|mut stream| {
        let app = app.clone();
        thread::spawn(move || { let mut bytes = [0u8; 1024]; while let Ok(n) = stream.read(&mut bytes) { if n == 0 { break; } send(&app, "progress", "installClaude", &String::from_utf8_lossy(&bytes[..n])); } })
    });
    let stderr_thread = stderr.map(|mut stream| {
        let app = app.clone();
        thread::spawn(move || { let mut bytes = [0u8; 1024]; while let Ok(n) = stream.read(&mut bytes) { if n == 0 { break; } send(&app, "progress", "installClaude", &String::from_utf8_lossy(&bytes[..n])); } })
    });
    let result = child.wait();
    if let Some(handle) = stdout_thread { let _ = handle.join(); }
    if let Some(handle) = stderr_thread { let _ = handle.join(); }
    let succeeded = result.is_ok_and(|s| s.success());
    if succeeded { send(&app, "done", "installClaude", "Claude Code installed."); }
    else { send(&app, "error", "installClaude", "Claude Code did not finish installing. See the installer's words above."); }
    remeasure(&app, succeeded);
}

/// A browser URL and the code prompt can arrive without a newline. This
/// parser only opens Anthropic's HTTPS hosts and never logs the URL.
fn login_output(app: &AppHandle, text: &str, opened: &mut bool, asked: &mut bool) {
    if !*opened {
        if let Some(url) = login_url(text) {
            if app.opener().open_url(url, None::<&str>).is_ok() {
                *opened = true;
                send(app, "browser", "signInClaude", "Finish signing in in the browser.");
            }
        }
    }
    if !*asked && text.to_ascii_lowercase().contains("paste code") {
        *asked = true;
        send(app, "code", "signInClaude", "Paste the code from your browser here.");
    }
}

fn terminal_login(app: &AppHandle, distro: &str) -> Result<(), String> {
    // Claude Code's OAuth callback and code fallback are terminal-oriented on
    // some WSL installations. Open a command already running; no typing a
    // shell command is required. Windows Terminal is preferred, wsl.exe's own
    // console window is the fallback.
    let command = "export PATH=\"$HOME/.local/bin:$PATH\"; claude auth login --claudeai";
    let launched = Command::new("wt.exe").args(["-w", "new", "wsl.exe", "-d", distro, "--", "sh", "-lc", command]).spawn();
    if launched.is_err() {
        Command::new("wsl.exe").args(["-d", distro, "--", "sh", "-lc", command]).spawn().map_err(|e| e.to_string())?;
    }
    send(app, "terminal", "signInClaude", "A sign-in window opened. Follow the browser, then paste a code there if Claude asks.");
    Ok(())
}

fn sign_in(app: AppHandle, distro: String) {
    let command = "export PATH=\"$HOME/.local/bin:$PATH\"; claude auth login --claudeai";
    let mut child = match wsl(&distro, None, &["sh", "-lc", command]).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped()).spawn() {
        Ok(child) => child, Err(error) => { app.state::<Controller>().setup_login_busy.store(false, Ordering::SeqCst); send(&app, "error", "signInClaude", &error.to_string()); remeasure(&app, false); return; }
    };
    if let Ok(mut slot) = app.state::<Controller>().setup_login.lock() { *slot = child.stdin.take(); }
    send(&app, "progress", "signInClaude", "Opening Claude sign-in…");
    let shared = Arc::new(Mutex::new((String::new(), false, false)));
    let readers = [child.stdout.take().map(|s| Box::new(s) as Box<dyn Read + Send>),
        child.stderr.take().map(|s| Box::new(s) as Box<dyn Read + Send>)];
    let handles: Vec<_> = readers.into_iter().flatten().map(|mut stream| {
        let app = app.clone(); let shared = shared.clone();
        thread::spawn(move || {
            let mut bytes = [0u8; 512];
            while let Ok(n) = stream.read(&mut bytes) {
                if n == 0 { break; }
                if let Ok(mut seen) = shared.lock() {
                    seen.0.push_str(&String::from_utf8_lossy(&bytes[..n]));
                    if seen.0.len() > 8192 {
                        let keep = seen.0.chars().count().saturating_sub(4096);
                        seen.0 = seen.0.chars().skip(keep).collect();
                    }
                    let text = seen.0.clone();
                    let (_, opened, asked) = &mut *seen;
                    login_output(&app, &text, opened, asked);
                }
            }
        })
    }).collect();
    let began = Instant::now();
    let outcome = loop {
        match child.try_wait() {
            Ok(Some(exit)) => break Some(exit.success()),
            Ok(None) if began.elapsed() >= Duration::from_secs(20) &&
                !shared.lock().map(|seen| seen.1 || seen.2).unwrap_or(false) => {
                let _ = child.kill(); let _ = child.wait(); break Some(false);
            }
            Ok(None) if began.elapsed() < Duration::from_secs(600) => thread::sleep(Duration::from_millis(250)),
            _ => { let _ = child.kill(); let _ = child.wait(); break None; }
        }
    };
    for handle in handles { let _ = handle.join(); }
    if let Ok(mut slot) = app.state::<Controller>().setup_login.lock() { *slot = None; }
    app.state::<Controller>().setup_login_busy.store(false, Ordering::SeqCst);
    match outcome {
        Some(true) => {
            let measured = direct_claude(&distro);
            if measured["signedIn"].as_bool() == Some(true) && measured["billing"].as_str() != Some("api") {
                send(&app, "done", "signInClaude", "Claude sign-in finished.");
            } else if measured["billing"].as_str() == Some("api") {
                send(&app, "error", "signInClaude", "Claude still reports Console/API billing after sign-in.");
            } else {
                send(&app, "error", "signInClaude", "Could not check Claude sign-in yet.");
            }
        }
        Some(false) => {
            let measured = direct_claude(&distro);
            if measured["signedIn"].as_bool() == Some(true) && measured["billing"].as_str() != Some("api") {
                send(&app, "done", "signInClaude", "Claude sign-in finished.");
            } else if let Err(error) = terminal_login(&app, &distro) {
                send(&app, "error", "signInClaude", &error);
            }
        }
        None => send(&app, "error", "signInClaude", "Claude sign-in timed out. Try again."),
    }
    remeasure(&app, outcome == Some(true));
}

#[tauri::command(async)]
pub fn setup_action(app: AppHandle, action: String, distro: Option<String>) -> Result<(), String> {
    match action.as_str() {
        "installWsl" => { thread::spawn(move || install_wsl(app)); Ok(()) }
        "openDistro" => {
            let distro = person_distro(distro)?;
            if crate::wsl::distro_running(&distro) { return Ok(()); }
            terminal_login_distro(&distro)?;
            remeasure(&app, true);
            Ok(())
        }
        "installClaude" | "signInClaude" => {
            let distro = person_distro(distro)?;
            if !crate::wsl::distro_running(&distro) { return Err("Open Linux first".into()); }
            let claude = direct_claude(&distro);
            if action == "installClaude" {
                if claude["installed"].as_bool() != Some(false) { return Err("Claude Code is not measured as missing".into()); }
                thread::spawn(move || install_claude(app, distro));
            } else {
                if claude["installed"].as_bool() != Some(true) ||
                    (claude["signedIn"].as_bool() != Some(false) && claude["billing"].as_str() != Some("api")) {
                    return Err("Claude Code is not measured as signed out or using Console/API billing".into());
                }
                if app.state::<Controller>().setup_login_busy.swap(true, Ordering::SeqCst) { return Err("Claude sign-in is already open".into()); }
                thread::spawn(move || sign_in(app, distro));
            }
            Ok(())
        }
        _ => Err("Unknown setup step".into()),
    }
}

fn terminal_login_distro(distro: &str) -> Result<(), String> {
    let launched = Command::new("wt.exe").args(["-w", "new", "wsl.exe", "-d", distro]).spawn();
    if launched.is_err() { Command::new("wsl.exe").args(["-d", distro]).spawn().map_err(|e| e.to_string())?; }
    Ok(())
}

#[tauri::command(async)]
pub fn setup_code(app: AppHandle, code: String) -> Result<(), String> {
    if code.trim().is_empty() || code.len() > 4096 || code.contains('\n') || code.contains('\r') {
        return Err("Paste the code Claude showed in your browser".into());
    }
    let controller = app.state::<Controller>();
    let mut slot = controller.setup_login.lock().map_err(|e| e.to_string())?;
    let stdin: &mut ChildStdin = slot.as_mut().ok_or("Claude is not asking for a code")?;
    stdin.write_all(code.trim().as_bytes()).map_err(|e| e.to_string())?;
    stdin.write_all(b"\n").map_err(|e| e.to_string())?;
    stdin.flush().map_err(|e| e.to_string())
}
