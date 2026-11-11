use super::*;
use crate::acp::{
    contract::{RequiredText, SafeText},
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::HostError,
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionControlError {
    #[error("the harness declined the session control")]
    Refused(Option<SafeText<512>>),
    #[error("the session cannot take this control now")]
    Unavailable,
    #[error("the session control outcome is unknown")]
    RecoveryRequired,
}

impl AcpService {
    /// Carries out a session control on the owner's running session; the
    /// text says what the harness did, when it says.
    pub(crate) fn control_session(
        &self,
        owner: RuntimeOwner,
        control: SessionControl,
    ) -> Result<Option<SafeText<512>>, SessionControlError> {
        let lane = self
            .lane_for(&owner)
            .ok_or(SessionControlError::Unavailable)?;
        let host = lane
            .runtime
            .for_owner(&owner)
            .map_err(|_| SessionControlError::Unavailable)?;
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::Control(control),
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::Deadline
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        match result {
            Ok(NativeRpcReply::Controlled(summary)) => Ok(summary),
            Ok(NativeRpcReply::ControlRefused(reason)) => Err(SessionControlError::Refused(reason)),
            Ok(NativeRpcReply::Rejected) => Err(SessionControlError::Refused(None)),
            Ok(_) => Err(SessionControlError::RecoveryRequired),
            Err(HostError::Intent | HostError::State) => Err(SessionControlError::Unavailable),
            Err(_) => Err(SessionControlError::RecoveryRequired),
        }
    }
}

/// Renames one call sends before the latest is known to be the one Pi has.
const RETITLE_PASSES: usize = 3;

impl AcpService {
    /// Names the chat's running harness session after the chat's title as
    /// Sage now keeps it, so the harness lists it by that name too. Reading
    /// the kept title rather than carrying one makes overlapping renames
    /// settle on the latest. Best effort: a chat with no running session, or
    /// a harness that names no sessions, keeps its title in Sage alone, and
    /// a name the harness did not confirm in time leaves the session as is.
    /// Recovery follows the same rules as any other session control.
    pub(crate) fn retitle_live_session(&self, project_id: &str, chat_id: &str) {
        let Some(owner) = self.lanes().find_map(|lane| {
            lane.runtime.owner().filter(|owner| {
                owner.project_id.as_str() == project_id && owner.chat_id.as_str() == chat_id
            })
        }) else {
            return;
        };
        // A rename made while this one was on its way is sent after it, so
        // the session ends named after the chat's latest title.
        let mut sent = None;
        for _ in 0..RETITLE_PASSES {
            let Some(title) = self.kept_chat_title(project_id, chat_id) else {
                return;
            };
            if sent.as_ref() == Some(&title) {
                return;
            }
            if self
                .control_session(
                    owner.clone(),
                    SessionControl::SetTitle {
                        title: title.clone(),
                    },
                )
                .is_err()
            {
                return;
            }
            sent = Some(title);
        }
    }

    fn kept_chat_title(
        &self,
        project_id: &str,
        chat_id: &str,
    ) -> Option<RequiredText<{ crate::acp::intent::session_event::SESSION_TITLE_BYTES }>> {
        let stored: String = self
            .database
            .connection()
            .query_row(
                "SELECT title FROM chats WHERE project_id=?1 AND id=?2",
                rusqlite::params![project_id, chat_id],
                |row| row.get(0),
            )
            .ok()?;
        RequiredText::new(
            "sessionTitle",
            crate::chat_lifecycle::chat_display_title(&stored),
        )
        .ok()
    }
}
