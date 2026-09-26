//! READING WHAT A DAEMON PRINTS as words a person can read.
//!
//! Split out of `lib.rs` (2026-09-26, SOLID F161). The supervisor and the
//! connect flow each carried their own copy of the stderr drain (keep the
//! last five readable lines); it has one home here now, next to `readable`.
//! Std-only, so it is tested without building the app:
//! `rustc --edition 2021 --test src/daemon_output.rs`.

use std::{io::{BufRead, BufReader}, process::ChildStderr, thread::{self, JoinHandle}};

/// One stderr line as a person can read it: wsl.exe's own errors arrive as
/// UTF-16 (NULs between the letters) and the daemon may colour its output.
pub(crate) fn readable(line: &str) -> String {
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

/// Drain a daemon's stderr on its own thread, so a chatty stderr can never
/// stall stdout; its last five readable lines are the reason when the process
/// ends without saying one.
pub(crate) fn stderr_tail(err: ChildStderr) -> JoinHandle<Vec<String>> {
    thread::spawn(move || {
        let mut tail: Vec<String> = Vec::new();
        for line in BufReader::new(err).lines().map_while(Result::ok) {
            let line = readable(&line);
            if line.is_empty() { continue; }
            tail.push(line);
            if tail.len() > 5 { tail.remove(0); }
        }
        tail
    })
}

#[cfg(test)]
mod tests {
    use super::readable;

    include!("source_pins.rs");

    #[test]
    fn stderr_lines_read_as_words() {
        assert_eq!(readable("\u{1b}[31merror:\u{1b}[0m no git repository "), "error: no git repository");
        assert_eq!(readable("W\0S\0L\0 \0e\0r\0r\0o\0r\0"), "WSL error");
    }

    /// The tail keeps the LAST five readable lines, blank and colour-only
    /// lines dropped.
    #[cfg(unix)]
    #[test]
    fn the_tail_keeps_the_last_five_words() {
        use std::process::{Command, Stdio};
        let mut child = Command::new("sh").args(["-c", "for n in 1 2 3 4 5 6 7; do echo \"line $n\" >&2; echo >&2; done; printf '\\033[0m\\n' >&2"])
            .stdout(Stdio::null()).stderr(Stdio::piped()).spawn().expect("sh");
        let tail = child.stderr.take().map(super::stderr_tail).expect("stderr").join().expect("drain");
        let _ = child.wait();
        assert_eq!(tail, vec!["line 3", "line 4", "line 5", "line 6", "line 7"]);
    }

    /// One home: the two readers of a daemon's stderr take the drain from
    /// here, and a hand-rolled copy coming back anywhere in `src/` fails this.
    /// The canary: this file's own code must match, or the walk proves nothing.
    #[test]
    fn the_drain_has_one_home() {
        let banned = ["tail.remove(0)", "fn readable("];
        let home = code(include_str!("daemon_output.rs"));
        for needle in banned { assert!(home.contains(needle), "canary: daemon_output.rs no longer holds {needle}"); }
        let sources = rust_sources();
        assert!(sources.len() >= 10 && sources.iter().any(|(name, _)| name == "daemon_output.rs"), "the walk found src/");
        for (name, source) in sources.iter().filter(|(name, _)| name != "daemon_output.rs") {
            let source = code(source);
            for needle in banned { assert!(!source.contains(needle), "{name} drains or decodes stderr by hand again ({needle})"); }
        }
        let (supervisor, connect) = (code(include_str!("supervisor.rs")), code(include_str!("connect.rs")));
        assert!(supervisor.contains("child.stderr.take().map(stderr_tail)"));
        assert!(connect.contains("child.stderr.take().map(stderr_tail)"));
    }
}
