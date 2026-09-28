//! Pure decisions for the Windows card's WSL route. I/O stays in setup.rs.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WslState { Missing, NoDistro, Ready, Unknown }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClaudeState { Missing, SignedOut, ApiBilling, Ready, Unknown }
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step { InstallWsl, OpenDistro, InstallClaude, SignInClaude }

pub fn classify_wsl(status_ok: bool, status: &str, list_ok: bool, list: &str, distros: &[String]) -> WslState {
    if !distros.is_empty() { return WslState::Ready; }
    let words = format!("{status} {list}").to_ascii_lowercase();
    if words.contains("not installed") || words.contains("not enabled") || words.contains("optional component") {
        return WslState::Missing;
    }
    if list_ok || words.contains("no installed distributions") || words.contains("no distributions installed") {
        return WslState::NoDistro;
    }
    if status_ok && words.contains("default distribution") { return WslState::NoDistro; }
    WslState::Unknown
}

pub fn wsl_steps(wsl: WslState, distro_running: bool, claude: ClaudeState) -> Vec<Step> {
    match wsl {
        WslState::Missing | WslState::NoDistro => vec![Step::InstallWsl],
        WslState::Ready if !distro_running => vec![Step::OpenDistro],
        WslState::Ready | WslState::Unknown if distro_running => match claude {
            ClaudeState::Missing => vec![Step::InstallClaude],
            ClaudeState::SignedOut | ClaudeState::ApiBilling => vec![Step::SignInClaude],
            ClaudeState::Ready | ClaudeState::Unknown => vec![],
        },
        WslState::Ready | WslState::Unknown => vec![],
    }
}

/// Every press, including a failed or cancelled one, asks the machine again.
/// Only the next measurement can remove a row.
pub fn remeasure_after_press(_succeeded: bool) -> bool { true }

pub fn login_url(text: &str) -> Option<&str> {
    for (start, _) in text.match_indices("https://") {
        let rest = &text[start..];
        let Some(end) = rest.find(|c: char| c.is_whitespace() || c == '\u{1b}' || c == '"' || c == '<' || c == '>') else { continue; };
        let url = &rest[..end];
        if ["https://claude.ai/", "https://claude.com/", "https://console.anthropic.com/", "https://platform.claude.com/"]
            .iter().any(|host| url.starts_with(host)) { return Some(url); }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_measured_missing_steps_are_offered_in_route_order() {
        assert_eq!(wsl_steps(WslState::Missing, false, ClaudeState::Unknown), [Step::InstallWsl]);
        assert_eq!(wsl_steps(WslState::NoDistro, false, ClaudeState::Unknown), [Step::InstallWsl]);
        assert_eq!(wsl_steps(WslState::Ready, false, ClaudeState::Unknown), [Step::OpenDistro]);
        assert_eq!(wsl_steps(WslState::Ready, true, ClaudeState::Missing), [Step::InstallClaude]);
        assert_eq!(wsl_steps(WslState::Ready, true, ClaudeState::SignedOut), [Step::SignInClaude]);
        assert_eq!(wsl_steps(WslState::Ready, true, ClaudeState::ApiBilling), [Step::SignInClaude]);
        assert!(wsl_steps(WslState::Ready, true, ClaudeState::Ready).is_empty());
        assert!(wsl_steps(WslState::Ready, true, ClaudeState::Unknown).is_empty());
        assert!(wsl_steps(WslState::Unknown, false, ClaudeState::Missing).is_empty());
        assert_eq!(wsl_steps(WslState::Unknown, true, ClaudeState::Missing), [Step::InstallClaude], "unknown WSL does not hide a measured missing CLI in a running distro");
    }

    #[test]
    fn a_press_never_assumes_success_and_always_remeasures() {
        assert!(remeasure_after_press(true));
        assert!(remeasure_after_press(false));
    }

    #[test]
    fn wsl_status_and_list_distinguish_absent_distro_from_an_unreadable_probe() {
        assert_eq!(classify_wsl(false, "WSL is not installed", false, "", &[]), WslState::Missing);
        assert_eq!(classify_wsl(true, "Default Version: 2", false, "Windows Subsystem for Linux has no installed distributions", &[]), WslState::NoDistro);
        assert_eq!(classify_wsl(true, "Default Version: 2", true, "", &[]), WslState::NoDistro);
        assert_eq!(classify_wsl(false, "", false, "", &[]), WslState::Unknown);
        assert_eq!(classify_wsl(false, "", true, "Ubuntu", &["Ubuntu".into()]), WslState::Ready);
    }

    #[test]
    fn browser_opens_only_a_complete_anthropic_https_url() {
        assert_eq!(login_url("Open https://claude.ai/oauth/authorize?code=abc\n"), Some("https://claude.ai/oauth/authorize?code=abc"));
        assert_eq!(login_url("Open https://claude.ai/oauth/authorize?code=abc"), None, "a chunk can end in the middle of a URL");
        assert_eq!(login_url("https://claude.ai.evil.test/login\n"), None);
        assert_eq!(login_url("http://claude.ai/login\n"), None);
        assert_eq!(login_url("See https://example.com/help then https://claude.ai/oauth/authorize?code=abc\n"), Some("https://claude.ai/oauth/authorize?code=abc"));
    }
}
