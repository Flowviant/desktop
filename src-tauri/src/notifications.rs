//! WHICH MACHINE EVENTS BECOME A NATIVE NOTIFICATION, and what each one says.
//!
//! Split out of `announce` (2026-09-26; `announce` now lives in
//! `supervisor.rs`). The allowlist used to be written twice: here in Rust,
//! which actually shows the notification, and as `shouldNotify` in
//! `src/model.ts`, which nothing in the app called — so its
//! passing test could never notice the native behaviour drifting. The
//! TypeScript copy is gone; this is the one policy, pure (no `AppHandle`, no
//! plugin) so its tests compile on their own: `rustc --test src/notifications.rs`.
//!
//! The tray app notifies only for MACHINE events (CLAUDE.md, Machines): a box
//! displaced, a daemon that stopped unexpectedly, a CLI limit, an applied
//! update. Agent and card activity never reach the OS.

/// What `announce` shows for one machine event.
#[derive(Debug, PartialEq, Eq)]
pub struct Notice {
    /// The daemon's own words when it sent any, else this app's sentence.
    pub body: String,
    /// Whether the tray icon turns to attention. An applied update is news,
    /// not a problem, so it notifies without asking for attention.
    pub attention: bool,
}

/// The notification for an event of `kind` carrying `message`, or `None` for
/// an event that is not a machine event worth interrupting anyone for.
pub fn notification_for_event(kind: &str, message: Option<&str>) -> Option<Notice> {
    let fallback = match kind {
        "displaced" => "This box stopped serving after another box took the machine",
        "stopped-unexpectedly" => "The daemon stopped unexpectedly",
        "update-applied" => "The daemon updated",
        "limit-hit" => "A CLI hit its limit",
        _ => return None,
    };
    Some(Notice {
        body: message.map(str::to_string).unwrap_or_else(|| fallback.into()),
        attention: kind != "update-applied",
    })
}

#[cfg(test)]
mod tests {
    use super::{notification_for_event, Notice};

    #[test]
    fn every_machine_event_notifies_in_its_own_words_or_ours() {
        let cases = [
            ("displaced", "This box stopped serving after another box took the machine", true),
            ("stopped-unexpectedly", "The daemon stopped unexpectedly", true),
            ("limit-hit", "A CLI hit its limit", true),
            ("update-applied", "The daemon updated", false),
        ];
        for (kind, fallback, attention) in cases {
            assert_eq!(notification_for_event(kind, None), Some(Notice { body: fallback.into(), attention }), "{kind}");
            assert_eq!(
                notification_for_event(kind, Some("Claude: usage limit reached")),
                Some(Notice { body: "Claude: usage limit reached".into(), attention }),
                "{kind}"
            );
        }
    }

    #[test]
    fn nothing_else_reaches_the_os() {
        for kind in ["serving", "stopped", "limit-cleared", "agent-turn", "card-delivered", ""] {
            assert_eq!(notification_for_event(kind, Some("words")), None, "{kind}");
        }
    }
}
