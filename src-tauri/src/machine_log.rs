//! THE APP'S OWN LOG (machines.log).
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161): the supervisor, the
//! connect flow, the switch and the updater all write here, and none of them
//! should own the file's rotation or its timestamps.

use std::fs;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

// ── The app's own log ────────────────────────────────────────────────────────
// Every decision this app makes about a daemon — start, exit (code, reason, its
// last words), standby, retry, WSL going away, updates, the on/off switch — one
// line each, in %LOCALAPPDATA%\com.flowviant.desktop\logs\machines.log, rotated
// at 1 MB to machines.log.1. The daemons keep their own logs in WSL
// (`flowviant logs`); this is the half those cannot see.

const LOG_MAX_BYTES: u64 = 1024 * 1024;

pub(crate) fn log_line(app: &AppHandle, line: &str) {
    let Ok(dir) = app.path().app_log_dir() else { return };
    let _ = fs::create_dir_all(&dir);
    let path = dir.join("machines.log");
    if fs::metadata(&path).map(|m| m.len() > LOG_MAX_BYTES).unwrap_or(false) {
        let _ = fs::rename(&path, dir.join("machines.log.1"));
    }
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(file, "{} {}", iso_utc(stamp), line.replace('\n', " / "));
    }
}

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ` (no date crate for one line).
fn iso_utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let (h, m, sec) = (rem / 3600, (rem % 3600) / 60, rem % 60);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let mo = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(mo <= 2);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{m:02}:{sec:02}Z")
}

#[tauri::command]
pub fn open_logs(app: AppHandle) -> Result<(), String> {
    let dir = app.path().app_log_dir().map_err(|e| e.to_string())?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    app.opener().open_path(dir.to_string_lossy().to_string(), None::<&str>).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::iso_utc;

    #[test]
    fn log_lines_are_stamped_in_utc() {
        assert_eq!(iso_utc(0), "1970-01-01T00:00:00Z");
        assert_eq!(iso_utc(1_790_391_031), "2026-09-26T02:50:31Z");
        assert_eq!(iso_utc(951_782_400), "2000-02-29T00:00:00Z");
    }
}
