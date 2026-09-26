//! UNINSTALLING: cleaning WSL on the way out, and the tray menu's door to it.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). `uninstall_cleanup` runs
//! from the NSIS pre-uninstall hook as `--uninstall-cleanup` (see `main.rs`),
//! with no Tauri app at all, so it reads the saved connections by their
//! Windows path and bounds every `wsl.exe` it starts by one deadline.

use std::{collections::HashSet, fs, path::PathBuf, process::{Command, Stdio}, thread, time::{Duration, Instant}};

use crate::connections::Connection;
use crate::wsl::{binary_path_for_home, parse_wsl_distros, quiet, wsl};

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

/// The same uninstaller Settings › Apps runs (NSIS writes it beside the exe).
/// It cleans WSL through the pre-uninstall hook; this app quits so the
/// uninstaller finds nothing running.
pub(crate) fn launch_uninstaller() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let uninstaller = exe.parent().ok_or("no install folder")?.join("uninstall.exe");
    if !uninstaller.is_file() { return Err("uninstaller not found".into()); }
    Command::new(uninstaller).spawn().map(|_| ()).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::merge_cleanup_distros;
    use crate::connections::Connection;

    #[test]
    fn cleanup_deduplicates_saved_and_discovered_distros() {
        let saved = vec![
            Connection { id: "first".into(), distro: "Ubuntu".into(), dir: "/one".into(), enabled: true, name: None },
            Connection { id: "second".into(), distro: "Ubuntu".into(), dir: "/two".into(), enabled: true, name: None },
            Connection { id: "third".into(), distro: "Debian".into(), dir: "/three".into(), enabled: true, name: None },
        ];
        assert_eq!(merge_cleanup_distros(&saved, vec!["Debian".into(), "Fedora".into(), "Fedora".into()]),
            vec!["Ubuntu", "Debian", "Fedora"]);
    }
}
