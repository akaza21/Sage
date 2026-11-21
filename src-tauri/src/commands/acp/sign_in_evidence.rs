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
