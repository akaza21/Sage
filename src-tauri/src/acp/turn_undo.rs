use crate::acp::{contract::SageTurnId, errors::SessionControlError, scheduler::RuntimeOwner};

/// Preparation boundary shared by native bindings: resolve first, then commit
/// the intent. No native effect is admitted until this returns successfully.
pub(crate) fn stage_turn_undo<E: std::fmt::Display>(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
    turn_id: SageTurnId,
    harness_reverted: bool,
    resolve_cut: impl FnOnce() -> Result<Option<String>, E>,
) -> Result<crate::acp::store::TurnUndoneCommand, SessionControlError> {
    let cut_at = if harness_reverted {
        None
    } else {
        resolve_cut().map_err(|error| {
            SessionControlError::Refused(
                crate::acp::contract::SafeText::new("controlSummary", error.to_string()).ok(),
            )
        })?
    };
    let command = crate::acp::store::TurnUndoneCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        turn_id,
        harness_reverted,
        cut_at,
        now_millis: sage_core::time::now_millis(),
    };
    let mut connection = database
        .write_connection()
        .map_err(|_| SessionControlError::RecoveryRequired)?;
    crate::acp::store::turn_undo::intent::start(&mut connection, &command)
        .map_err(|_| SessionControlError::RecoveryRequired)?;
    Ok(command)
}
