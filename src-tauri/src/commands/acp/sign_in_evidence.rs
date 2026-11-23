use super::*;

pub(crate) async fn send_acp_turn_inner(
    state: &AppState,
    request: SendAcpTurnRequest,
) -> Result<AcpMutationResponse, SendAcpTurnFailure> {
    let (project, _, chat) = send_owner(&request);
    let (project, chat) = (project.clone(), chat.clone());
    let (intent, session) = match &request {
        SendAcpTurnRequest::Session {
            client_intent_id,
            session_id,
            ..
        } => (client_intent_id.clone(), Some(session_id.clone())),
        SendAcpTurnRequest::NewSession {
            client_intent_id, ..
        } => (client_intent_id.clone(), None),
    };
    // A clean refusal while Sage switches branches. The store re-checks the
    // lease inside the PrepareTurn commit, which is the actual guarantee.
    let leased = crate::repository_mutation_guard::project_leased(
        &state.database.connection(),
        project.as_str(),
    )
    .map_err(|error| {
        session_operations::prompt_not_dispatched(errors::store_error(
            crate::acp::store::StoreError::Database(error),
        ))
    })?;
    if leased {
        return Err(session_operations::prompt_not_dispatched(
            errors::store_error(crate::acp::store::StoreError::RepositoryBusy),
        ));
    }
    session_operations::send_acp_turn(state, request)
        .await
        .map_err(|mut failure| {
            failure.failure = errors::persisted_sign_in_error(
                state,
                crate::acp::store::commands::sign_in_evidence::RefusalCommand {
                    project_id: project.as_str(),
                    chat_id: chat.as_str(),
                    client_intent_id: intent.as_str(),
                    session_id: session.as_ref().map(|id| id.as_str()),
                },
                failure.failure,
            );
            failure
        })
}
