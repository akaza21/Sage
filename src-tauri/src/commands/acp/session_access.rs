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
    let chat = state
        .settings
        .chat_access(session_id.as_str())
        .map_err(|_| access_unreadable())?;
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
