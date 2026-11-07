use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::acp::{
    contract::{EntityRevision, RuntimeEpoch, SageAgentSessionId},
    intent::{
        ModelId,
        session_event::{
            SessionActivity, SessionActivityState, SessionNotice, SessionNoticeLevel,
            SessionSurface,
        },
    },
};

/// Renderer view of one session's session-scoped state. Live fields (model
/// and activities) appear only while the epoch that reported them runs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionSurfaceProjection {
    pub session_id: SageAgentSessionId,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub notices: Vec<SessionNoticeProjection>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_id: Option<ModelId>,
    pub activities: Vec<SessionActivityProjection>,
    pub revision: EntityRevision,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionNoticeProjection {
    pub level: SessionNoticeLevel,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionActivityProjection {
    /// Opaque, session-scoped digest of the harness identity.
    pub key: String,
    pub kind: String,
    pub label: String,
    pub state: SessionActivityState,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<String>,
}

impl SessionSurfaceProjection {
    pub(crate) fn project(
        session_id: SageAgentSessionId,
        surface: SessionSurface,
        running_epoch: Option<RuntimeEpoch>,
        revision: EntityRevision,
    ) -> Self {
        let live = running_epoch.is_some_and(|epoch| epoch.get() == surface.live_epoch);
        let activities = if live {
            surface
                .activities
                .into_iter()
                .map(|activity| project_activity(&session_id, activity))
                .collect()
        } else {
            Vec::new()
        };
        Self {
            title: surface.title.map(|title| title.as_str().to_owned()),
            notices: surface.notices.into_iter().map(project_notice).collect(),
            model_id: surface.model_id.filter(|_| live),
            activities,
            revision,
            session_id,
        }
    }
}

fn project_notice(notice: SessionNotice) -> SessionNoticeProjection {
    SessionNoticeProjection {
        level: notice.level,
        text: notice.text.as_str().to_owned(),
        source: notice.source.map(|source| source.as_str().to_owned()),
    }
}

fn project_activity(
    session_id: &SageAgentSessionId,
    activity: SessionActivity,
) -> SessionActivityProjection {
    let mut digest = Sha256::new();
    digest.update(session_id.as_str().as_bytes());
    digest.update([0]);
    digest.update(activity.id.as_str().as_bytes());
    SessionActivityProjection {
        key: format!("{:x}", digest.finalize()),
        kind: activity.kind.as_str().to_owned(),
        label: activity.label.as_str().to_owned(),
        state: activity.state,
        progress: activity
            .progress
            .map(|progress| progress.as_str().to_owned()),
    }
}
