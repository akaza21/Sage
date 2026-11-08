use serde::Serialize;

use crate::acp::{
    contract::RequiredText,
    intent::session_event::{
        SESSION_ACTIVITY_KIND_BYTES, SESSION_ACTIVITY_LABEL_BYTES, SESSION_ACTIVITY_PROGRESS_BYTES,
        SessionActivity, SessionActivityState,
    },
};

/// A session activity row (a hook run, say) shown in the work list of the
/// Turn that last reported it. The harness's own row identity stays native.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSessionActivityProjection {
    pub kind: RequiredText<SESSION_ACTIVITY_KIND_BYTES>,
    pub label: RequiredText<SESSION_ACTIVITY_LABEL_BYTES>,
    pub state: SessionActivityState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<RequiredText<SESSION_ACTIVITY_PROGRESS_BYTES>>,
}

impl From<SessionActivity> for TurnSessionActivityProjection {
    fn from(row: SessionActivity) -> Self {
        Self {
            kind: row.kind,
            label: row.label,
            state: row.state,
            progress: row.progress,
        }
    }
}
