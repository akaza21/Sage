//! A chat's own access mode: reading it, choosing a narrower one, and
//! confirming a permissive one. A narrower choice applies to the live
//! session at once when no turn is running; stepping down from Full access
//! or Auto while a turn runs cancels that turn, so nothing keeps running
//! with access the person just took away.

use super::native_confirm::{self, ConfirmDialog, ConfirmGate, Refusal, Timing};
use super::*;
use crate::acp::bindings::access_policy::lists_mode;
use crate::project_registry::ProjectRegistryStore;
use sage_acp_protocol::contract::ClientIntentId;
use tauri::Manager;

#[derive(Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ChatAccessChange {
    /// A turn running with the access the chat stepped down from was
    /// cancelled; the person is told so.
    pub turn_cancelled: bool,
}

/// Whether the binding carries `mode` for a chat. A mode that needs a
/// confirmation must be offered by the bound build; any other only has to
/// be in the binding's own table, so a chat can always step down from Full
/// access or Auto, even while its harness is unavailable.
pub(super) fn carries_mode(
    registry: &crate::acp::bindings::registry::BindingRegistry,
    binding_id: &str,
    mode: SessionAccessMode,
) -> bool {
    if mode.needs_confirmation() {
        offers_mode(registry, binding_id, mode)
    } else {
        lists_mode(registry, binding_id, mode)
    }
}

/// The binding of an existing session that carries `mode`.
fn chat_binding(
    state: &AppState,
    session_id: &SageAgentSessionId,
    mode: SessionAccessMode,
) -> Result<String, AcpIpcError> {
    let binding_id = session_routes::durable_binding_id(state, session_id)
        .map_err(|_| AcpIpcError::InvalidRequest {
            message: safe_message("This session no longer exists."),
        })?
        .as_str()
        .to_owned();
    if !carries_mode(state.acp.binding_registry(), &binding_id, mode) {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("This harness does not offer the chosen access mode."),
        });
    }
    Ok(binding_id)
}

/// The chat's own access mode, if it has one.
#[tauri::command]
pub(crate) fn get_chat_access(
    state: State<'_, AppState>,
    request: ChatAccessRequest,
) -> Result<Option<SessionAccessMode>, AcpIpcError> {
    state
        .settings
        .chat_access(request.session_id.as_str())
        .map_err(|_| access_unreadable())
}

/// Sets a chat's mode when it needs no confirmation. It replaces any
/// permissive mode the chat had, and reaches the live session under the
/// project transition lock `configure` takes, off the async runtime.
#[tauri::command]
pub(crate) async fn set_chat_access(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ChooseChatAccessRequest,
) -> Result<ChatAccessChange, AcpIpcError> {
    let binding_id = chat_binding(&state, &request.session_id, request.mode)?;
    if request.mode.needs_confirmation() {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("This access mode needs a confirmation."),
        });
    }
    let session = request.session_id.as_str();
    let previous = state
        .settings
        .chat_access(session)
        .map_err(|_| access_unreadable())?;
    state
        .settings
        .set_chat_access(session, request.mode, sage_core::time::now_millis())
        .map_err(|_| access_unreadable())?;
    let stepped_down = previous.is_some_and(SessionAccessMode::needs_confirmation);
    let transition = state.project_transitions.lock().await;
    let Ok(generation) = ProjectGeneration::new(*transition) else {
        return Ok(ChatAccessChange::default());
    };
    let Some(owner) = state
        .acp
        .live_owner_of(&request.session_id)
        .filter(|owner| {
            state.database.active_project_id().ok().flatten().as_deref()
                == Some(owner.project_id.as_str())
        })
    else {
        return Ok(ChatAccessChange::default());
    };
    let mode = request.mode;
    let native = session_mode_for_binding(state.acp.binding_registry(), &binding_id, mode);
    let change = tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        let host = LiveChat {
            state: &state,
            generation,
        };
        apply_live_chat_access(&host, owner, &binding_id, mode, native, stepped_down)
    })
    .await
    .unwrap_or_default();
    drop(transition);
    Ok(change)
}

/// A live chat session, reached through the ACP service.
pub(super) trait LiveChatHost: SessionModeHost {
    /// Cancels the session's running turn; false when none is running.
    fn cancel_running_turn(&self, owner: &RuntimeOwner) -> Result<bool, AcpIpcError>;
}

struct LiveChat<'a> {
    state: &'a AppState,
    generation: ProjectGeneration,
}

impl SessionModeHost for LiveChat<'_> {
    fn live_config(&self, owner: &RuntimeOwner) -> Option<SessionConfigOverlay> {
        self.state.acp.live_config(owner)
    }
    fn configure(&self, intent: ConfigIntent) -> Result<(), HostError> {
        SessionModeHost::configure(&self.state.acp, intent)
    }
}

impl LiveChatHost for LiveChat<'_> {
    fn cancel_running_turn(&self, owner: &RuntimeOwner) -> Result<bool, AcpIpcError> {
        let Some(turn_id) = self
            .state
            .acp
            .cancellable_turn(owner, self.generation)
            .map_err(store_error)?
        else {
            return Ok(false);
        };
        let binding_id = session_routes::durable_binding_id(self.state, &owner.session_id)
            .map_err(store_error)?;
        self.state
            .acp
            .dispatch_session_cancel(
                &binding_id,
                crate::acp::runtime::HostCancelIntent {
                    owner: owner.clone(),
                    turn_id,
                    client_intent_id: ClientIntentId::new(),
                    now_millis: sage_core::time::now_millis(),
                },
            )
            .map_err(dispatch_error)?;
        Ok(true)
    }
}

/// Moves a live session to the chat's new mode. After a step-down from a
/// permissive mode a running turn is cancelled first; the next prompt then
/// selects the new mode, as it always does.
pub(super) fn apply_live_chat_access(
    host: &impl LiveChatHost,
    owner: RuntimeOwner,
    binding_id: &str,
    mode: SessionAccessMode,
    native: Option<&'static str>,
    stepped_down: bool,
) -> ChatAccessChange {
    if stepped_down {
        match host.cancel_running_turn(&owner) {
            Ok(true) => {
                tracing::warn!(
                    session_id = owner.session_id.as_str(),
                    binding_id,
                    ?mode,
                    "A turn running with permissive access was cancelled after a step-down"
                );
                return ChatAccessChange {
                    turn_cancelled: true,
                };
            }
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(?error, "A step-down could not cancel the running turn");
            }
        }
    }
    if let Err(error) = apply_session_mode(host, owner, binding_id, mode, native) {
        // A running turn keeps its mode; the next prompt selects this one.
        tracing::debug!(?error, "Chat access applies from the next prompt");
    }
    ChatAccessChange::default()
}

/// Opens the confirmation of a permissive mode and returns its token.
#[tauri::command]
pub(crate) fn prepare_chat_access(
    state: State<'_, AppState>,
    request: ChooseChatAccessRequest,
) -> Result<AccessConfirmation, AcpIpcError> {
    chat_binding(&state, &request.session_id, request.mode)?;
    if !request.mode.needs_confirmation() {
        return Err(AcpIpcError::InvalidRequest {
            message: safe_message("This access mode needs no confirmation."),
        });
    }
    let confirm_token = state
        .settings
        .prepare_chat_access(
            request.session_id.as_str(),
            request.mode,
            sage_core::time::now_millis(),
        )
        .map_err(|_| access_unreadable())?;
    Ok(AccessConfirmation {
        confirm_token,
        expires_in_ms: crate::settings_store::ACCESS_CONFIRMATION_MILLIS,
    })
}

/// Spends a confirmation to give the chat a permissive mode, once the person
/// has also allowed it in a native dialog.
#[tauri::command]
pub(crate) async fn grant_chat_access(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: GrantChatAccessRequest,
) -> Result<(), AcpIpcError> {
    let binding_id = chat_binding(&state, &request.session_id, request.mode)?;
    grant_confirmed(
        &state,
        &native_confirm::NativeDialog(&app),
        &native_confirm::NATIVE_GATE,
        Timing::NATIVE,
        &binding_id,
        request,
    )
    .await
}

fn confirmation_missing() -> AcpIpcError {
    AcpIpcError::Conflict {
        resource: crate::acp::ipc::AcpConflictResource::ConfigRevision,
        message: safe_message("This confirmation is missing, used or expired. Confirm again."),
    }
}

/// The grant, asking through `dialog`: only for a token that is still
/// pending, and only when the person allows it there.
pub(super) async fn grant_confirmed(
    state: &AppState,
    dialog: &impl ConfirmDialog,
    gate: &ConfirmGate,
    timing: Timing,
    binding_id: &str,
    request: GrantChatAccessRequest,
) -> Result<(), AcpIpcError> {
    let session = request.session_id.as_str();
    let token = request.confirm_token.as_str();
    if !request.mode.needs_confirmation() {
        return Err(AcpIpcError::Rejected {
            message: safe_message("This access was not allowed."),
        });
    }
    let pending = state
        .settings
        .chat_access_pending(session, request.mode, token, sage_core::time::now_millis())
        .map_err(|_| access_unreadable())?;
    if !pending {
        return Err(confirmation_missing());
    }
    let label = native_confirm::session_label(state, &request.session_id);
    let text = native_confirm::dialog_text(request.mode, &label);
    if let Err(refusal) = native_confirm::ask(gate, dialog, text, timing).await {
        tracing::info!(
            session_id = session,
            ?refusal,
            "Permissive access was not allowed"
        );
        return Err(AcpIpcError::Rejected {
            message: safe_message(match refusal {
                Refusal::Busy => "Another access confirmation is already open.",
                _ => "This access was not allowed.",
            }),
        });
    }
    state
        .settings
        .grant_chat_access(session, request.mode, token, sage_core::time::now_millis())
        .map_err(|_| confirmation_missing())?;
    tracing::warn!(
        session_id = session,
        binding_id,
        mode = ?request.mode,
        "Permissive access mode granted to a chat session"
    );
    Ok(())
}
