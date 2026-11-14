//! A runtime that still holds a turn the user undid takes no prompt (the
//! store refuses to prepare one), so the send relaunches the session without
//! the turn first and goes to the new runtime.

use super::*;
use crate::acp::{intent::AcpConflictResource, service::UndoneRelaunchError};

/// Runs under the project transition lock `transition`, as an activation does.
pub(super) async fn relaunch_undone(
    state: &AppState,
    transition: &crate::state::ActiveProjectContext<'_>,
    request: &mut SendAcpTurnRequest,
) -> Result<(), AcpIpcError> {
    let SendAcpTurnRequest::Session {
        project_id,
        project_generation,
        chat_id,
        expected_projection_revision,
        session_id,
        runtime_epoch,
        ..
    } = request
    else {
        return Ok(());
    };
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session_id.clone(),
        runtime_epoch: *runtime_epoch,
    };
    if !state.acp.holds_undone_turn(&owner).map_err(store_error)? {
        return Ok(());
    }
    let binding_id = session_routes::durable_binding_id(state, session_id).map_err(store_error)?;
    let admitted = state
        .acp
        .resolve_session_binding(&binding_id)
        .map_err(|_| binding_unavailable())?;
    let project_root = admitted
        .project_root(&state.database, transition)
        .map_err(|_| binding_unavailable())?;
    let relaunched = state
        .acp
        .relaunch_without_undone_turn(admitted, &owner, *project_generation, project_root)
        .await
        .map_err(|error| match error {
            UndoneRelaunchError::Parking(refusal) => {
                tracing::warn!(?refusal, "a runtime holding an undone turn was not parked");
                AcpIpcError::Conflict {
                    resource: AcpConflictResource::Session,
                    message: safe_message(
                        "The agent is still restarting after the undo; send again.",
                    ),
                }
            }
            UndoneRelaunchError::Activation(error) => session_error(error),
        })?;
    // The park and the activation moved the page on; the prompt was
    // validated against the page the user saw just before them.
    let snapshot = state
        .acp
        .intent_snapshot(project_id.clone(), *project_generation, chat_id.clone())
        .map_err(store_error)?;
    *expected_projection_revision = snapshot.projection_revision;
    *session_id = relaunched.session_id;
    *runtime_epoch = relaunched.runtime_epoch;
    Ok(())
}
