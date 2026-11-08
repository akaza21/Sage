//! One rule for turning a harness-reported session title into a session
//! event, shared by every binding that receives one.

use crate::acp::{
    contract::RequiredText,
    intent::session_event::{SESSION_TITLE_BYTES, SessionScopedEvent},
};

/// The first line of `raw` with control characters removed, trimmed and cut
/// at a character boundary within [`SESSION_TITLE_BYTES`]. `None` when nothing
/// printable is left, so a harness can clear or blank a title only by being
/// ignored.
pub(crate) fn harness_title_event(raw: &str) -> Option<SessionScopedEvent> {
    let line = raw.lines().next()?;
    let mut title = String::new();
    for character in line.trim().chars().filter(|value| !value.is_control()) {
        if title.len() + character.len_utf8() > SESSION_TITLE_BYTES {
            break;
        }
        title.push(character);
    }
    let title = title.trim_end();
    RequiredText::new("sessionTitle", title)
        .ok()
        .map(|title| SessionScopedEvent::Title { title })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn title(raw: &str) -> Option<String> {
        harness_title_event(raw).map(|event| match event {
            SessionScopedEvent::Title { title } => title.as_str().to_owned(),
            other => panic!("expected a title, got {other:?}"),
        })
    }

    #[test]
    fn keeps_the_first_printable_line_within_the_cap() {
        assert_eq!(title("  Fix login \nsecond"), Some("Fix login".into()));
        assert_eq!(title("Tab\there\u{1b}"), Some("Tabhere".into()));
        assert_eq!(title(""), None);
        assert_eq!(title(" \u{7} "), None);
        assert_eq!(title("\nlater line"), None);
        let long = title(&"é".repeat(SESSION_TITLE_BYTES)).unwrap();
        assert_eq!(long.len(), SESSION_TITLE_BYTES);
        assert!(long.chars().all(|value| value == 'é'));
    }
}
