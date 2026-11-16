//! The access mode a prompt runs under, and each chat's own choice of it.
//! A binding's stored default (Settings) covers every chat; a chat's choice
//! applies to its session only. The permissive modes (Full access, Auto)
//! take a confirmed two-step command and are never a default. A binding
//! that enforces access through harness session modes (Claude Code's
//! permission modes) gets the matching mode selected before each prompt, so
//! a mode the harness or an older runtime left behind never outlives the
//! chat's choice. Nothing here depends on how a harness names its modes:
//! that mapping lives with the binding (`access_policy::session_mode`).

use super::session_config::map_config_error;
use super::*;
use crate::acp::{
    bindings::access_policy::{offers_mode, session_mode_for_binding, turn_mode_for_binding},
    runtime::HostError,
    service::AcpService,
};
use sage_acp_protocol::access_mode::SessionAccessMode;
use sage_acp_protocol::{
    contract::{RequiredText, SageAgentSessionId},
    session_config::{ConfigIntent, ModeSelection, SessionConfigOverlay},
};
use serde::{Deserialize, Serialize};

pub(crate) mod chat;
mod native_confirm;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChatAccessRequest {
    pub session_id: SageAgentSessionId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ChooseChatAccessRequest {
    pub session_id: SageAgentSessionId,
    pub mode: SessionAccessMode,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GrantChatAccessRequest {
    pub session_id: SageAgentSessionId,
    pub mode: SessionAccessMode,
    pub confirm_token: RequiredText<64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct AccessConfirmation {
    pub confirm_token: String,
    pub expires_in_ms: i64,
}

fn access_unreadable() -> AcpIpcError {
    AcpIpcError::RecoveryRequired {
        message: safe_message("Session access mode could not be loaded; reload Sage."),
    }
}

/// The access a prompt in `session_id` runs under: the chat's own choice
/// while its binding still offers it, else the binding's stored default.
pub(super) fn turn_access_mode(
    state: &AppState,
    binding_id: &str,
    session_id: &SageAgentSessionId,
) -> Result<SessionAccessMode, AcpIpcError> {
    let chat = session_chat_access(state, session_id)?;
    let defaults = state
        .settings
        .session_access_modes()
        .map_err(|_| access_unreadable())?;
    turn_mode_for_binding(
        state.acp.binding_registry(),
        binding_id,
        chat,
        defaults.get(binding_id).copied(),
    )
    .ok_or_else(|| AcpIpcError::InvalidRequest {
        message: safe_message("Choose a supported access mode before sending."),
    })
}

/// The harness session mode a Turn the harness starts on its own in
/// `owner`'s session may run in: the one the access a prompt there would
/// run under maps to now. None when that cannot be read, or the binding
/// does not carry access in session modes.
pub(crate) fn harness_turn_mode(state: &AppState, owner: &RuntimeOwner) -> Option<&'static str> {
    let binding = session_routes::durable_binding_id(state, &owner.session_id).ok()?;
    let mode = turn_access_mode(state, binding.as_str(), &owner.session_id).ok()?;
    session_mode_for_binding(state.acp.binding_registry(), binding.as_str(), mode)
}

/// The chat's own access of `session_id`, after it took over what it
/// inherits: every read of a session's access, for the composer and for a
/// prompt alike, goes through here, so both see the same mode.
pub(super) fn session_chat_access(
    state: &AppState,
    session_id: &SageAgentSessionId,
) -> Result<Option<SessionAccessMode>, AcpIpcError> {
    carry_chat_access(state, session_id)?;
    state
        .settings
        .chat_access(session_id.as_str())
        .map_err(|_| access_unreadable())
}

/// Gives a session that has run no turn the access of the session its chat
/// ran just before it on the same binding, so the chat's choice outlives a
/// failed session, a resume on a later harness release and a restart after
/// an undo. The two live in different stores, so nothing copies them in one
/// transaction: which session goes on from which is durable with the
/// sessions themselves, and the copy is made again wherever it is missing,
/// before any read of the session's access. A session that went on from one
/// whose copy never happened (Sage stopped during its activation) takes the
/// access from the session before that, each in between taking it too.
/// Idempotent: a session that holds a mode of its own keeps it.
///
/// The walk goes back through the chat's own sessions, each earlier than the
/// last, so it ends within the chat's session limit. A walk that does not
/// (sessions beyond that limit) refuses, so a prompt never runs under a
/// binding default broader than the chat's choice.
pub(super) fn carry_chat_access(
    state: &AppState,
    session_id: &SageAgentSessionId,
) -> Result<(), AcpIpcError> {
    // From `session_id` back to the first session that holds a mode, or that
    // goes on from none.
    let mut chain = vec![session_id.clone()];
    let mut resolved = false;
    while chain.len() <= sage_core::limits::LIMITS.durable.sessions_per_chat as usize {
        let last = chain.last().expect("the chain starts with the session");
        let held = state
            .settings
            .chat_access(last.as_str())
            .map_err(|_| access_unreadable())?;
        let earlier = match held {
            Some(_) => None,
            None => crate::acp::store::preceding_session(&state.database.connection(), last)
                .map_err(|_| access_unreadable())?,
        };
        let Some(earlier) = earlier else {
            resolved = true;
            break;
        };
        chain.push(earlier);
    }
    if !resolved {
        return Err(access_unreadable());
    }
    let now = sage_core::time::now_millis();
    for pair in chain.windows(2).rev() {
        state
            .settings
            .carry_chat_access(pair[1].as_str(), pair[0].as_str(), now)
            .map_err(|_| access_unreadable())?;
    }
    Ok(())
}

/// The access a prompt runs under, selected in its live session first.
pub(super) fn select_turn_access(
    state: &AppState,
    binding_id: &str,
    owner: RuntimeOwner,
) -> Result<SessionAccessMode, AcpIpcError> {
    let mode = turn_access_mode(state, binding_id, &owner.session_id)?;
    select_session_mode(state, owner, binding_id, mode)?;
    Ok(mode)
}

/// The live session config a mode is selected through.
pub(super) trait SessionModeHost {
    fn live_config(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay>;
    fn configure(&self, intent: ConfigIntent) -> Result<(), HostError>;
}

impl SessionModeHost for AcpService {
    fn live_config(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay> {
        self.live_session_config(owner)
    }
    fn configure(&self, intent: ConfigIntent) -> Result<(), HostError> {
        self.configure_session(intent)
    }
}

/// Selects the harness session mode that carries `mode` before a prompt.
/// Bindings without session-mode enforcement are left alone. A mode the live
/// session does not advertise is refused, never approximated, and any error
/// stops the prompt before it is sent.
pub(super) fn select_session_mode(
    state: &AppState,
    owner: RuntimeOwner,
    binding_id: &str,
    mode: SessionAccessMode,
) -> Result<(), AcpIpcError> {
    let native = session_mode_for_binding(state.acp.binding_registry(), binding_id, mode);
    apply_session_mode(&state.acp, owner, binding_id, mode, native)
}

pub(super) fn apply_session_mode(
    host: &impl SessionModeHost,
    owner: RuntimeOwner,
    binding_id: &str,
    mode: SessionAccessMode,
    native: Option<&'static str>,
) -> Result<(), AcpIpcError> {
    let session_id = owner.session_id.clone();
    let Some(native) = native else {
        record_turn_access(&session_id, binding_id, mode, None, None);
        return Ok(());
    };
    let overlay = host
        .live_config(&owner)
        .filter(|overlay| !overlay.removed)
        .ok_or_else(|| AcpIpcError::InvalidRequest {
            message: safe_message(
                "This session has not reported its permission modes yet. Try again in a moment.",
            ),
        })?;
    let (previous, mutation) = match overlay.config.select_mode(native) {
        ModeSelection::Current => {
            record_turn_access(&session_id, binding_id, mode, Some(native), Some(native));
            return Ok(());
        }
        ModeSelection::NotOffered => {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message(
                    "This session does not offer the chosen access mode. Choose another one.",
                ),
            });
        }
        ModeSelection::Change { previous, mutation } => (previous, mutation),
    };
    host.configure(ConfigIntent {
        owner,
        expected_config_revision: overlay.config_revision,
        mutation,
    })
    .map_err(map_config_error)?;
    record_turn_access(
        &session_id,
        binding_id,
        mode,
        Some(native),
        previous.as_deref(),
    );
    Ok(())
}

/// Turn diagnostics: every turn in a permissive mode is recorded, whether
/// or not the session was already in it, and so is every mode switch.
fn record_turn_access(
    session_id: &SageAgentSessionId,
    binding_id: &str,
    mode: SessionAccessMode,
    session_mode: Option<&str>,
    previous_mode: Option<&str>,
) {
    let switched = session_mode.is_some() && session_mode != previous_mode;
    if mode.needs_confirmation() {
        tracing::warn!(
            session_id = session_id.as_str(),
            binding_id,
            ?mode,
            session_mode,
            previous_mode,
            switched,
            "Permissive access mode for this turn"
        );
    } else if switched {
        tracing::info!(
            session_id = session_id.as_str(),
            binding_id,
            ?mode,
            session_mode,
            previous_mode,
            "Access mode selected for this turn"
        );
    }
}

#[cfg(test)]
#[path = "session_access_tests.rs"]
mod tests;
