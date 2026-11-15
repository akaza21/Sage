use super::*;
use crate::acp::{
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::HostError,
};
use sage_acp_protocol::{
    contract::{RequiredText, SafeText},
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

impl AcpService {
    pub(crate) fn control_session(
        &self,
        owner: RuntimeOwner,
        control: SessionControl,
    ) -> Result<Option<SafeText<512>>, SessionControlError> {
        match self.session_control_reply(owner, control)? {
            NativeRpcReply::Controlled(summary) => Ok(summary),
            _ => Err(SessionControlError::RecoveryRequired),
        }
    }

    /// A control that abandons at its deadline (see
    /// `SessionControl::abandons_at_deadline`) is late, not a reason to recover the
    /// runtime.
    pub(in crate::acp::service) fn session_control_reply(
        &self,
        owner: RuntimeOwner,
        control: SessionControl,
    ) -> Result<NativeRpcReply, SessionControlError> {
        let abandons = control.abandons_at_deadline();
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
        if let Err(error) = &result
            && needs_recovery(error, abandons)
        {
            self.retain_config_effect_recovery(owner, host);
        }
        match result {
            Ok(NativeRpcReply::ControlRefused(reason)) => Err(SessionControlError::Refused(reason)),
            Ok(NativeRpcReply::Rejected) => Err(SessionControlError::Refused(None)),
            Ok(reply) => Ok(reply),
            Err(HostError::Intent | HostError::State) => Err(SessionControlError::Unavailable),
            Err(HostError::Deadline) if abandons => Err(SessionControlError::Late),
            Err(_) => Err(SessionControlError::RecoveryRequired),
        }
    }
}

/// Whether a failed control or native read leaves the runtime's state
/// unknown. One its binding abandons at the deadline changed nothing Sage
/// holds.
pub(super) fn needs_recovery(error: &HostError, abandons: bool) -> bool {
    match error {
        HostError::Unavailable
        | HostError::Closed
        | HostError::DeliveryUnknown
        | HostError::RecoveryRequired => true,
        HostError::Deadline => !abandons,
        _ => false,
    }
}

#[cfg(test)]
#[test]
fn only_an_abandoned_read_misses_its_deadline_without_recovery() {
    assert!(!needs_recovery(&HostError::Deadline, true));
    assert!(needs_recovery(&HostError::Deadline, false));
    assert!(needs_recovery(&HostError::DeliveryUnknown, true));
    assert!(!needs_recovery(&HostError::Intent, false));
    assert!(!needs_recovery(&HostError::State, false));
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
    ) -> Option<RequiredText<{ sage_acp_protocol::intent::session_event::SESSION_TITLE_BYTES }>>
    {
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
