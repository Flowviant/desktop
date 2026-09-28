//! TALKING TO WSL: every `wsl.exe` this app starts, and the facts it reads back.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). One place builds the
//! command (quiet, `-d <distro>`, `--cd`), decodes what `wsl.exe` prints
//! (UTF-16 or UTF-8), maps a picked `\\wsl$` folder to a distro and path, and
//! answers "which distros are running" without ever booting one. It is
//! std-only on purpose, so its rules are tested without building the app:
//! `rustc --edition 2021 --test src/wsl.rs`.

use std::{collections::HashSet, process::Command};

/// Every wsl.exe this app starts is a console program; from a GUI process each
/// one would flash its own console window unless told not to.
#[cfg_attr(not(windows), allow(unused_mut))]
pub(crate) fn quiet(mut cmd: Command) -> Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

pub(crate) fn wsl(distro: &str, dir: Option<&str>, args: &[&str]) -> Command {
    let mut cmd = quiet(Command::new("wsl.exe"));
    cmd.args(["-d", distro]);
    if let Some(dir) = dir { cmd.args(["--cd", dir]); }
    cmd.arg("--").args(args);
    cmd
}

pub(crate) fn run_wsl(distro: &str, dir: Option<&str>, args: &[&str]) -> Result<String, String> {
    let output = wsl(distro, dir, args).output().map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub(crate) fn parse_wsl_folder(path: &str) -> Result<(String, String), String> {
    let normalized;
    let path = if path.to_ascii_lowercase().starts_with("\\\\?\\unc\\") {
        normalized = format!("\\\\{}", &path[8..]);
        normalized.as_str()
    } else { path };
    let parts: Vec<_> = path.split('\\').filter(|s| !s.is_empty()).collect();
    if !path.starts_with("\\\\") || parts.len() < 3 || !matches!(parts[0].to_ascii_lowercase().as_str(), "wsl$" | "wsl.localhost") {
        return Err("Flowviant runs inside WSL — pick your project folder inside it".into());
    }
    let distro = parts[1];
    if !distro.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)) || parts[2..].iter().any(|s| *s == "." || *s == "..") {
        return Err("Pick a folder inside a WSL distro".into());
    }
    Ok((distro.into(), format!("/{}", parts[2..].join("/"))))
}

pub(crate) fn version(v: &str) -> Vec<u64> {
    v.trim().trim_start_matches('v').split('.').take(3).map(|s| s.parse().unwrap_or(0)).collect()
}

pub(crate) fn binary_path(distro: &str) -> Result<String, String> {
    Ok(binary_path_for_home(&run_wsl(distro, None, &["sh", "-c", "printf %s \"$HOME\""])?))
}

pub(crate) fn binary_path_for_home(home: &str) -> String { format!("{home}/.flowviant/bin/flowviant") }

pub(crate) fn decode_wsl(output: &[u8]) -> String {
    if output.starts_with(&[0xff, 0xfe]) || (output.len() >= 2 && output[1] == 0) {
        String::from_utf16_lossy(&output.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect::<Vec<_>>())
    } else { String::from_utf8_lossy(output).to_string() }
}

pub(crate) fn parse_wsl_distros(output: &[u8]) -> Vec<String> {
    decode_wsl(output).lines().map(|line| line.trim_matches(['\u{feff}', '\0', '\r', ' ']).to_string())
        .filter(|name| !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))).collect()
}

/// Distros WSL has RUNNING right now. `--list --running` asks the WSL service
/// and never boots a distro, which is the point: after `wsl --shutdown` this app
/// must not start WSL again on its own (the owner, 2026-09-25).
pub(crate) fn running_distros() -> HashSet<String> {
    let Ok(output) = quiet(Command::new("wsl.exe")).args(["--list", "--running", "--quiet"]).output() else { return HashSet::new() };
    parse_wsl_distros(&output.stdout).into_iter().filter(|name| !name.starts_with("docker-desktop")).collect()
}

pub(crate) fn distro_running(distro: &str) -> bool { running_distros().contains(distro) }

#[cfg(test)]
mod tests {
    use super::{binary_path_for_home, parse_wsl_distros, parse_wsl_folder, version};

    #[test]
    fn wsl_distro_list_decodes_utf16_and_ignores_invalid_names() {
        let output: Vec<u8> = "\u{feff}Ubuntu\r\nBad Name\r\nDebian\r\n".encode_utf16().flat_map(u16::to_le_bytes).collect();
        assert_eq!(parse_wsl_distros(&output), vec!["Ubuntu", "Debian"]);
        assert_eq!(parse_wsl_distros(b"Ubuntu\nDebian\n"), vec!["Ubuntu", "Debian"]);
    }

    #[test]
    fn a_picked_folder_names_its_distro_and_linux_path() {
        assert_eq!(parse_wsl_folder("\\\\wsl.localhost\\Ubuntu\\home\\me\\app"), Ok(("Ubuntu".into(), "/home/me/app".into())));
        assert_eq!(parse_wsl_folder("\\\\wsl$\\Debian\\srv"), Ok(("Debian".into(), "/srv".into())));
        assert_eq!(parse_wsl_folder("\\\\?\\UNC\\wsl.localhost\\Ubuntu\\home"), Ok(("Ubuntu".into(), "/home".into())));
        assert_eq!(parse_wsl_folder("C:\\code\\app"), Err("Flowviant runs inside WSL — pick your project folder inside it".into()));
        assert_eq!(parse_wsl_folder("\\\\wsl$\\Ubuntu\\home\\..\\etc"), Err("Pick a folder inside a WSL distro".into()));
        assert_eq!(parse_wsl_folder("\\\\wsl$\\Bad Name\\home"), Err("Pick a folder inside a WSL distro".into()));
    }

    #[test]
    fn versions_compare_by_number_and_the_binary_lives_under_home() {
        assert!(version("v0.102.2\n") > version("0.99.0"));
        assert!(version("0.102.2") >= version("0.102.2"));
        assert_eq!(binary_path_for_home("/home/me"), "/home/me/.flowviant/bin/flowviant");
    }
}
