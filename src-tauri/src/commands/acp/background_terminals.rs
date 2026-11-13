use super::*;
use crate::acp::{
    background_terminals::{BackgroundTerminal, TerminalAction, valid_handle},
    service::background_terminals::{TerminalsError, absent_runtime},
};
use usage::{AcpUsageRequest, current_owner};

/// The commands the current runtime's harness left running. A runtime that
/// is no longer current runs none.
#[tauri::command]
pub(crate) async fn get_background_terminals(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: AcpUsageRequest,
) -> Result<Vec<BackgroundTerminal>, AcpIpcError> {
    run(app, &state, request, TerminalAction::List).await
}

/// One listed terminal by its handle, or all of them without one.
#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct StopTerminalsRequest {
    runtime: AcpUsageRequest,
    handle: Option<String>,
}

/// Stops the named terminals; the caller lists again for what still runs.
#[tauri::command]
pub(crate) async fn stop_background_terminals(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: StopTerminalsRequest,
) -> Result<(), AcpIpcError> {
    let action = match request.handle {
        Some(handle) if valid_handle(&handle) => TerminalAction::Stop(handle),
        Some(_) => {
            return Err(AcpIpcError::InvalidRequest {
                message: safe_message("The background terminal handle is invalid."),
            });
        }
        None => TerminalAction::StopAll,
    };
    run(app, &state, request.runtime, action).await.map(drop)
}

async fn run(
    app: tauri::AppHandle,
    state: &AppState,
    request: AcpUsageRequest,
    action: TerminalAction,
) -> Result<Vec<BackgroundTerminal>, AcpIpcError> {
    let result = match current_owner(state, request).await? {
        Some(owner) => tauri::async_runtime::spawn_blocking(move || {
            use tauri::Manager;
            app.state::<AppState>()
                .acp
                .background_terminals(owner, action)
        })
        .await
        .map_err(|_| internal_error())?,
        None => absent_runtime(&action),
    };
    result.map_err(|error| match error {
        TerminalsError::Stale => intent_error(IntentError::StaleRuntimeEpoch),
        TerminalsError::Rejected => AcpIpcError::Rejected {
            message: safe_message("The harness did not take the background terminal request."),
        },
        TerminalsError::RecoveryRequired => AcpIpcError::RecoveryRequired {
            message: safe_message(
                "The background terminal request outcome is unknown; reload the session.",
            ),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::contract::{RuntimeEpoch, SageAgentSessionId};
    use crate::commands::acp::goal::integration_tests::fixture;
    use std::sync::atomic::Ordering;

    #[tokio::test]
    async fn a_listing_and_a_stop_reach_only_the_current_runtime() {
        let (state, goal, calls) = fixture(Ok(()));
        let request = |epoch| AcpUsageRequest {
            project_id: goal.project_id.clone(),
            project_generation: goal.project_generation,
            chat_id: goal.chat_id.clone(),
            session_id: goal.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        };
        assert_eq!(current_owner(&state, request(2)).await.unwrap(), None);
        let owner = current_owner(&state, request(1)).await.unwrap().unwrap();
        let listed = state
            .acp
            .background_terminals(owner.clone(), TerminalAction::List)
            .unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].command, "npm run dev");
        assert_eq!(
            state
                .acp
                .background_terminals(owner.clone(), TerminalAction::StopAll),
            Ok(Vec::new())
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        let foreign = RuntimeOwner {
            session_id: SageAgentSessionId::parse("foreign-session").unwrap(),
            ..owner
        };
        assert_eq!(
            state
                .acp
                .background_terminals(foreign.clone(), TerminalAction::List)
                .unwrap(),
            Vec::new()
        );
        // A stop aimed at a runtime that is gone reached nothing.
        assert_eq!(
            state
                .acp
                .background_terminals(foreign, TerminalAction::StopAll),
            Err(TerminalsError::Stale)
        );
        assert_eq!(calls.load(Ordering::SeqCst), 2);
        state.acp.shutdown_blocking().unwrap();
    }
}
