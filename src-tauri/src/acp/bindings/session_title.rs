//! One rule for turning a harness-reported session title into a session
//! event, shared by every binding that receives one.

use crate::acp::{
    contract::RequiredText,
    intent::session_event::{SESSION_TITLE_BYTES, SessionScopedEvent},
};

/// The harness title as a session event, sanitized by the same rule as every
/// chat title and capped at [`SESSION_TITLE_BYTES`]. `None` when nothing
/// printable is left: unreadable input is ignored, never taken as a clear.
pub(crate) fn harness_title_event(raw: &str) -> Option<SessionScopedEvent> {
    let title = crate::chat_lifecycle::title_line(raw, SESSION_TITLE_BYTES)?;
    RequiredText::new("sessionTitle", title)
        .ok()
        .map(|title| SessionScopedEvent::Title { title: Some(title) })
}

/// The event for a harness that explicitly removed its session title. The
/// chat falls back to the title Sage would have given it, unless the user
/// named it or it is a fork.
#[cfg_attr(not(test), allow(dead_code))]
pub(crate) fn harness_title_cleared() -> SessionScopedEvent {
    SessionScopedEvent::Title { title: None }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(raw: &str) -> Option<String> {
        harness_title_event(raw).map(|event| match event {
            SessionScopedEvent::Title { title: Some(title) } => title.as_str().to_owned(),
            other => panic!("expected a title, got {other:?}"),
        })
    }

    #[test]
    fn keeps_the_first_printable_line_within_the_cap() {
        assert_eq!(title("  Fix login \nsecond"), Some("Fix login".into()));
        assert_eq!(title("Tab\there\u{1b}"), Some("Tabhere".into()));
        assert_eq!(title("\u{7} \u{202e}Spoof"), Some("Spoof".into()));
        assert_eq!(title(""), None);
        assert_eq!(title(" \u{7} "), None);
        assert_eq!(title("\nlater line"), None);
        let long = title(&"é".repeat(SESSION_TITLE_BYTES)).unwrap();
        assert_eq!(long.len(), SESSION_TITLE_BYTES);
        assert!(long.chars().all(|value| value == 'é'));
    }
}
