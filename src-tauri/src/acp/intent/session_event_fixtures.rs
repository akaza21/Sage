//! Test constructors for session-scoped events.

use super::*;

pub(crate) fn notice(level: SessionNoticeLevel, text: &str) -> SessionScopedEvent {
    SessionScopedEvent::Notice(SessionNotice {
        level,
        text: RequiredText::new("noticeText", text).unwrap(),
        source: None,
    })
}

pub(crate) fn title(text: &str) -> SessionScopedEvent {
    SessionScopedEvent::Title {
        title: RequiredText::new("title", text).unwrap(),
    }
}

pub(crate) fn activity(id: &str, state: SessionActivityState, progress: &str) -> SessionActivity {
    SessionActivity {
        id: ExternalId::new("activityId", id).unwrap(),
        kind: RequiredText::new("activityKind", "backgroundTask").unwrap(),
        label: RequiredText::new("activityLabel", "npm test").unwrap(),
        state,
        progress: Some(RequiredText::new("activityProgress", progress).unwrap()),
        turn_id: None,
    }
}
