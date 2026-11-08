//! The access mode a prompt runs under, and the per-session Full access
//! grant. A binding's stored default covers every chat; Full access is
//! granted to one session after a confirmed two-step command and is never a
//! default. A binding that enforces access through harness session modes
//! (Claude Code's permission modes) gets the matching mode selected before
//! each prompt, so a mode the harness or an older runtime left behind never
//! outlives the user's choice.

use super::session_config::map_config_error;
use super::*;
use crate::acp::{
    bindings::access_policy::{
        offers_full_access, session_mode_for_binding, turn_mode_for_binding,
    },
    contract::{RequiredText, SageAgentSessionId},
    session_config::{ConfigIntent, ModeSelection},
};
use crate::settings_store::SessionAccessMode;
use serde::{Deserialize, Serialize};

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct SessionFullAccessRequest {
    pub session_id: SageAgentSessionId,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct GrantSessionFullAccessRequest {
    pub session_id: SageAgentSessionId,
    pub confirm_token: RequiredText<64>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FullAccessConfirmation {
    pub confirm_token: String,
    pub expires_in_ms: i64,
}

fn access_unreadable() -> AcpIpcError {
    AcpIpcError::RecoveryRequired {
        message: safe_message("Session access mode could not be loaded; reload Sage."),
    }
}

fn full_access_admitted(state: &AppState, binding_id: &str) -> bool {
    offers_full_access(state.acp.binding_registry(), binding_id)
}

/// The access a prompt in `session_id` runs under: the session's Full access
/// grant when its binding still offers it, else the binding's stored default.
pub(super) fn turn_access_mode(
    state: &AppState,
    binding_id: &str,
    session_id: &SageAgentSessionId,
) -> Result<SessionAccessMode, AcpIpcError> {
    let granted = state
        .settings
        .has_full_access(session_id.as_str())
        .map_err(|_| access_unreadable())?;
    let defaults = state
        .settings
        .session_access_modes()
        .map_err(|_| access_unreadable())?;
    turn_mode_for_binding(
        state.acp.binding_registry(),
        binding_id,
        granted,
        defaults.get(binding_id).copied(),
    )
    .ok_or_else(|| AcpIpcError::InvalidRequest {
        message: safe_message("Choose a supported access mode before sending."),
    })
}

/// Selects the harness session mode that carries `mode` before a prompt.
/// Bindings without session-mode enforcement are left alone. A mode the live
/// session does not advertise is refused, never approximated.
pub(super) fn select_session_mode(
    state: &AppState,
    owner: RuntimeOwner,
    binding_id: &str,
    mode: SessionAccessMode,
) -> Result<(), AcpIpcError> {
    let Some(native) = session_mode_for_binding(state.acp.binding_registry(), binding_id, mode)
    else {
        return Ok(());
    };
    let overlay = state
        .acp
        .live_session_config(&owner)
        .filter(|overlay| !overlay.removed)
        .ok_or_else(|| AcpIpcError::InvalidRequest {
            message: safe_message(
                "This session has not reported its permission modes yet. Try again in a moment.",
            ),
        })?;
    let (previous, mutation) = match overlay.config.select_mode(native) {
        ModeSelection::Current => return Ok(()),
        ModeSelection::NotOffered => {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message(
                    "This session does not offer the chosen access mode. Choose another one.",
                ),
            });
        }
        ModeSelection::Change { previous, mutation } => (previous.unwrap_or_default(), mutation),
    };
    let session_id = owner.session_id.clone();
    state
        .acp
        .configure_session(ConfigIntent {
            owner,
            expected_config_revision: overlay.config_revision,
            mutation,
        })
        .map_err(map_config_error)?;
    // Turn diagnostics: every switch, and Full access above all, is recorded.
    if mode == SessionAccessMode::FullAccess {
        tracing::warn!(
            session_id = session_id.as_str(),
            binding_id,
            previous_mode = previous.as_str(),
            session_mode = native,
            "Full access selected for this turn"
        );
    } else {
        tracing::info!(
            session_id = session_id.as_str(),
            binding_id,
            previous_mode = previous.as_str(),
            session_mode = native,
            "Access mode selected for this turn"
        );
    }
    Ok(())
}

fn full_access_binding(
    state: &AppState,
    session_id: &SageAgentSessionId,
) -> Result<String, AcpIpcError> {
    let binding_id = session_routes::durable_binding_id(state, session_id)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("This session no longer exists."),
        })?
        .as_str()
        .to_owned();
    if !full_access_admitted(state, &binding_id) {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("This harness does not offer Full access."),
        });
    }
    Ok(binding_id)
}

#[tauri::command]
pub(crate) fn get_session_full_access(
    state: State<'_, AppState>,
) -> Result<Vec<String>, AcpIpcError> {
    state
        .settings
        .full_access_sessions()
        .map_err(|_| access_unreadable())
}

/// Opens a Full access confirmation for the session and returns its token.
#[tauri::command]
pub(crate) fn prepare_session_full_access(
    state: State<'_, AppState>,
    request: SessionFullAccessRequest,
) -> Result<FullAccessConfirmation, AcpIpcError> {
    full_access_binding(&state, &request.session_id)?;
    let confirm_token = state
        .settings
        .prepare_full_access(
            request.session_id.as_str(),
            crate::persistence::now_millis(),
        )
        .map_err(|_| access_unreadable())?;
    Ok(FullAccessConfirmation {
        confirm_token,
        expires_in_ms: crate::settings_store::FULL_ACCESS_CONFIRMATION_MILLIS,
    })
}

/// Grants Full access to the session. It applies from the next prompt.
#[tauri::command]
pub(crate) fn grant_session_full_access(
    state: State<'_, AppState>,
    request: GrantSessionFullAccessRequest,
) -> Result<(), AcpIpcError> {
    let binding_id = full_access_binding(&state, &request.session_id)?;
    state
        .settings
        .grant_full_access(
            request.session_id.as_str(),
            request.confirm_token.as_str(),
            crate::persistence::now_millis(),
        )
        .map_err(|_| AcpIpcError::Conflict {
            resource: crate::acp::intent::AcpConflictResource::ConfigRevision,
            message: safe_message(
                "This Full access confirmation is missing, used or expired. Confirm again.",
            ),
        })?;
    tracing::warn!(
        session_id = request.session_id.as_str(),
        binding_id = binding_id.as_str(),
        "Full access granted to a chat session"
    );
    Ok(())
}

#[tauri::command]
pub(crate) fn revoke_session_full_access(
    state: State<'_, AppState>,
    request: SessionFullAccessRequest,
) -> Result<(), AcpIpcError> {
    if state
        .settings
        .revoke_full_access(request.session_id.as_str())
        .map_err(|_| access_unreadable())?
    {
        tracing::info!(
            session_id = request.session_id.as_str(),
            "Full access ended for a chat session"
        );
    }
    Ok(())
}
