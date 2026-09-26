//! CONNECTING A FOLDER: the device-code sign-in that binds a WSL folder to
//! a project, and the page that approves it.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). `connect` runs
//! `flowviant login --json` in the picked folder, opens only a code this app
//! validated (or an app.flowviant.com URL), saves the bound connection and
//! hands it to the supervisor; `open_connect_page` is the card's door for a
//! code a person pasted.

use serde_json::{json, Value};
use std::{io::{BufRead, BufReader}, process::Stdio, thread};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::bundled_daemon::ensure_binary;
use crate::connections::{save_connections, Connection};
use crate::daemon_output::{readable, stderr_tail};
use crate::machine_log::log_line;
use crate::supervisor::start_one;
use crate::supervisor_rules;
use crate::wsl::{binary_path, parse_wsl_folder, wsl};
use crate::Controller;

/// A device code as `flowviant login` prints it (`MJBG-JCX75H`).
fn valid_code(code: &str) -> bool {
    !code.is_empty() && code.len() <= 16 && code.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

/// The web page that takes the code, asks which project, and approves it.
fn connect_url(code: &str) -> String { format!("https://app.flowviant.com/connect?code={code}") }

#[tauri::command(async)]
pub fn open_connect_page(app: AppHandle, code: String) -> Result<(), String> {
    if !valid_code(&code) { return Err("That code isn't one Flowviant printed".into()); }
    app.opener().open_url(connect_url(&code), None::<&str>).map_err(|e| e.to_string())
}

#[tauri::command(async)]
pub fn connect(app: AppHandle, folder: String) -> Result<(), String> {
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
        let stderr = child.stderr.take().map(stderr_tail);
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
                            let row = Connection { id: id.into(), distro: distro.clone(), dir: dir.clone(), enabled: true, name: event["name"].as_str().map(str::to_string) };
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
        log_line(&app, &format!("connect {distro}:{dir}: login ended (completed: {completed}){}", tail.last().map(|l| format!(" — {l}")).unwrap_or_default()));
        if !completed {
            // The supervisor's own words for an exit (its last stderr line,
            // else its code, zero included, else that it exited).
            let reason = supervisor_rules::exit_reason(tail.last().map(String::as_str), status.ok().and_then(|s| s.code()));
            fail(format!("Connection was not completed: {reason}"));
        }
        let _ = app.emit("status-changed", ());
    });
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{connect_url, valid_code};

    #[test]
    fn only_a_printed_code_reaches_the_connect_page() {
        assert!(valid_code("MJBG-JCX75H"));
        assert_eq!(connect_url("MJBG-JCX75H"), "https://app.flowviant.com/connect?code=MJBG-JCX75H");
        assert!(!valid_code(""));
        assert!(!valid_code("MJBG&next=https://evil"));
        assert!(!valid_code("ABCDEFGHIJKLMNOPQRSTUVWXYZ"));
    }
}
