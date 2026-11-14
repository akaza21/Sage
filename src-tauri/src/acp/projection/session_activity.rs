use serde::Serialize;

use sage_acp_protocol::{
    contract::RequiredText,
    intent::session_event::{
        SESSION_ACTIVITY_KIND_BYTES, SESSION_ACTIVITY_LABEL_BYTES, SESSION_ACTIVITY_PROGRESS_BYTES,
        SessionActivityState,
    },
};

/// The harness's own row identity stays native.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TurnSessionActivityProjection {
    pub kind: RequiredText<SESSION_ACTIVITY_KIND_BYTES>,
    pub label: RequiredText<SESSION_ACTIVITY_LABEL_BYTES>,
    pub state: SessionActivityState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<RequiredText<SESSION_ACTIVITY_PROGRESS_BYTES>>,
}
