use super::*;

pub(crate) async fn start_acp_session_inner(
    state: &AppState,
    request: StartAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    refuse_archived_chat(state, &request.project_id, &request.chat_id)?;
    validate_base_intent(
        &request.project_id,
        request.project_generation,
        &request.chat_id,
        request.expected_projection_revision,
        &snapshot,
    )
    .map_err(intent_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&request.binding_id) {
        return characterization::start(state, request).await;
    }
    let admitted = state
        .acp
        .resolve_session_binding(&request.binding_id)
        .map_err(|_| binding_unavailable())?;
    let project_root = admitted
        .project_root(&state.database, &transition)
        .map_err(|_| root_unavailable())?;
    // A thinking level chosen before the session existed must be one this
    // binding lists for the chosen model; it is refused otherwise, so the
    // chat never starts with a level the user did not get.
    let effort_choice = match request.effort.as_ref() {
        Some(effort) => Some(
            admitted
                .launch_effort_choice(
                    request.model_id.as_ref().map(|model| model.as_str()),
                    effort.as_str(),
                )
                .ok_or_else(|| {
                    AcpIpcError::InvalidRequest {
                        message: safe_message(
                            "The chosen thinking level is not offered for this model. Choose it again and send.",
                        ),
                    }
                })?,
        ),
        None => None,
    };
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let started = state
        .acp
        .start_session(
            admitted,
            &project_root,
            StartSessionCommand {
                project_id: request.project_id,
                chat_id: request.chat_id,
                expected_projection_revision: request.expected_projection_revision,
                client_intent_id: request.client_intent_id,
                binding_id: request.binding_id,
                binding_digest: String::new(),
                harness_version: None,
                model_id: request.model_id,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .await
        .map_err(session_error)?;
    if let Some(choice) = effort_choice
        && !state
            .acp
            .remember_start_choice(&started.session_id, &choice)
    {
        return Err(AcpIpcError::Internal {
            message: safe_message("The chosen thinking level could not be kept. Send again."),
        });
    }
    mutation_response(state, project_id, chat_id)
}

pub(crate) async fn activate_acp_session_inner(
    state: &AppState,
    request: ActivateAcpSessionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let (transition, snapshot) = snapshot_for(
        state,
        request.project_id.clone(),
        request.project_generation,
        request.chat_id.clone(),
    )
    .await?;
    refuse_archived_chat(state, &request.project_id, &request.chat_id)?;
    validate_session_intent(&request, &snapshot).map_err(intent_error)?;
    let binding_id = durable_binding_id(state, &request.session_id).map_err(store_error)?;
    #[cfg(all(windows, feature = "acp-fixture"))]
    if state.acp.accepts_characterization(&binding_id) {
        let project_root = state
            .database
            .selected_project_root_authority(&transition)
            .map_err(|_| AcpIpcError::Unavailable {
                reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
                message: safe_message(
                    "The selected project root is unavailable or changed; reselect it and retry.",
                ),
            })?;
        return characterization::activate(state, request, project_root).await;
    }
    let admitted = state
        .acp
        .resolve_session_binding(&binding_id)
        .map_err(|_| binding_unavailable())?;
    let project_root = admitted
        .project_root(&state.database, &transition)
        .map_err(|_| binding_unavailable())?;
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    state
        .acp
        .activate_session(
            admitted,
            ActivateSessionCommand {
                project_id: request.project_id,
                chat_id: request.chat_id,
                expected_projection_revision: request.expected_projection_revision,
                client_intent_id: request.client_intent_id,
                session_id: request.session_id,
                runtime_epoch: request.runtime_epoch,
                artifact_digest: String::new(),
                now_millis: crate::persistence::now_millis(),
            },
            project_root,
        )
        .await
        .map_err(session_error)?;
    mutation_response(state, project_id, chat_id)
}

fn root_unavailable() -> AcpIpcError {
    AcpIpcError::Unavailable {
        reason: crate::acp::intent::AcpUnavailableReason::CapabilityUnavailable,
        message: safe_message("Codex beta requires one unchanged Project root."),
    }
}

pub(super) fn durable_binding_id(
    state: &AppState,
    session_id: &crate::acp::contract::SageAgentSessionId,
) -> Result<crate::acp::intent::BindingId, crate::acp::store::StoreError> {
    let binding_id: String = state
        .database
        .connection()
        .query_row(
            "SELECT binding_id FROM agent_sessions WHERE id=?1",
            [session_id.as_str()],
            |row| row.get(0),
        )
        .map_err(crate::acp::store::StoreError::Database)?;
    crate::acp::intent::BindingId::new("bindingId", binding_id)
        .map_err(|_| crate::acp::store::StoreError::Corrupt("session binding identity"))
}

/// An archived Chat runs no agent. Prompt, session start and activation
/// refuse it here with a typed message while holding the project transition
/// lock, which archive also takes. Archive advances the transition epoch, so
/// a send that releases the lock for its workspace capture fails its relock
/// in `lock_active_project` as stale rather than dispatching.
pub(super) fn refuse_archived_chat(
    state: &AppState,
    project_id: &SageProjectId,
    chat_id: &SageChatId,
) -> Result<(), AcpIpcError> {
    match crate::chat_archive::is_archived_chat(
        &state.database.connection(),
        project_id.as_str(),
        chat_id.as_str(),
    ) {
        Ok(false) => Ok(()),
        Ok(true) => Err(archived_chat()),
        Err(_) => Err(internal_error()),
    }
}

/// A session's stored capabilities, read without loading a chat page. A
/// session outside the chat answers `None`, as when it was absent from the
/// chat's page.
pub(super) fn session_capabilities(
    state: &AppState,
    project_id: &crate::acp::contract::SageProjectId,
    chat_id: &crate::acp::contract::SageChatId,
    session_id: &crate::acp::contract::SageAgentSessionId,
) -> Result<Option<crate::acp::intent::CapabilitySnapshotV1>, crate::acp::store::StoreError> {
    use rusqlite::OptionalExtension;
    let capabilities: Option<String> = state
        .database
        .connection()
        .query_row(
            "SELECT capabilities_json FROM agent_sessions WHERE id=?1 AND project_id=?2 AND chat_id=?3",
            [session_id.as_str(), project_id.as_str(), chat_id.as_str()],
            |row| row.get(0),
        )
        .optional()
        .map_err(crate::acp::store::StoreError::Database)?;
    capabilities
        .map(|capabilities| serde_json::from_str(&capabilities))
        .transpose()
        .map_err(|_| crate::acp::store::StoreError::Corrupt("capabilities"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        contract::{ClientIntentId, RequiredText, RuntimeEpoch, SageChatId, SageProjectId},
        intent::{BindingId, CapabilitySnapshotV1, ExternalId},
        store::{
            ActivateSessionCommand, CompleteActivationCommand, StartSessionCommand,
            activate_session, complete_activation, start_session,
        },
    };

    #[test]
    fn prompt_resources_come_from_the_session_row_of_that_chat() {
        let root = std::env::temp_dir().join(format!("sage-resources-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let chat = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let state = AppState::new(database);
        let project_id = SageProjectId::parse(&project.id).unwrap();
        let chat_id = SageChatId::parse(&chat).unwrap();
        let revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let session = start_session(
            &mut state.database.connection(),
            StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("resources-start").unwrap(),
                binding_id: BindingId::new("binding", "sage-simulator").unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
        )
        .unwrap()
        .session_id;
        let accepts = |chat: &SageChatId| {
            session_capabilities(&state, &project_id, chat, &session)
                .unwrap()
                .is_some_and(|capabilities| capabilities.prompt_resources)
        };
        assert!(!accepts(&chat_id), "a pending session advertises nothing");

        let revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let epoch = RuntimeEpoch::new(1).unwrap();
        let mut connection = state.database.connection();
        activate_session(
            &mut connection,
            ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("resources-activate").unwrap(),
                session_id: session.clone(),
                runtime_epoch: epoch,
                artifact_digest: "a".repeat(64),
                now_millis: 2,
            },
        )
        .unwrap();
        let mut capabilities: CapabilitySnapshotV1 = serde_json::from_str(
            &connection
                .query_row(
                    "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                    [session.as_str()],
                    |row| row.get::<_, String>(0),
                )
                .unwrap(),
        )
        .unwrap();
        capabilities.protocol_version = RequiredText::new("protocol", "1").unwrap();
        capabilities.prompt_resources = true;
        complete_activation(
            &mut connection,
            CompleteActivationCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session.clone(),
                runtime_epoch: epoch,
                binding_digest: "a".repeat(64),
                artifact_digest: "a".repeat(64),
                external_session_id: ExternalId::new("session", "adapter-session").unwrap(),
                capabilities_json: serde_json::to_string(&capabilities).unwrap(),
                now_millis: 3,
            },
        )
        .unwrap();
        drop(connection);
        assert!(accepts(&chat_id));
        let other_chat = SageChatId::parse("chat-elsewhere").unwrap();
        assert!(
            !accepts(&other_chat),
            "a session of another chat never qualifies"
        );
    }
}
