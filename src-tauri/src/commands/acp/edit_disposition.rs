//! Owner-landed fail-closed disposition command. AGENT-1 implements the store write.

use tauri::{AppHandle, Emitter, State};

use crate::{
    acp::{
        contract::{EntityRevision, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId},
        intent::{
            ACP_PROJECTION_CHANGED_EVENT, AcpIpcError, AcpMutationResponse, ProjectionChangedV1,
        },
        store::edit_disposition::{
            AgentEditDisposition, AgentEditDispositionState, record_agent_edit_disposition_in,
        },
    },
    state::AppState,
};

use super::{
    errors::{internal_error, store_error},
    mutation_response,
};

#[derive(Debug, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecordAgentEditDispositionRequest {
    pub project_id: SageProjectId,
    pub chat_id: SageChatId,
    pub session_id: SageAgentSessionId,
    pub turn_id: SageTurnId,
    pub expected_revision: EntityRevision,
    pub state: AgentEditDispositionState,
}

#[tauri::command]
pub async fn record_agent_edit_disposition(
    app: AppHandle,
    state: State<'_, AppState>,
    request: RecordAgentEditDispositionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let session_id = request.session_id.clone();
    let response = record_agent_edit_disposition_inner(&state, request)?;
    app.emit(
        ACP_PROJECTION_CHANGED_EVENT,
        projection_changed(project_id, chat_id, session_id, &response),
    )
    .map_err(|_| internal_error())?;
    Ok(response)
}

fn record_agent_edit_disposition_inner(
    state: &AppState,
    request: RecordAgentEditDispositionRequest,
) -> Result<AcpMutationResponse, AcpIpcError> {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    {
        let mut connection = state.database.connection();
        record_agent_edit_disposition_in(
            &mut connection,
            &AgentEditDisposition {
                project_id: request.project_id,
                chat_id: request.chat_id,
                session_id: request.session_id,
                turn_id: request.turn_id,
                expected_revision: request.expected_revision,
                state: request.state,
            },
            crate::persistence::now_millis(),
        )
        .map_err(store_error)?;
    }
    mutation_response(state, project_id, chat_id)
}

fn projection_changed(
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    response: &AcpMutationResponse,
) -> ProjectionChangedV1 {
    ProjectionChangedV1 {
        project_id,
        chat_id,
        session_id,
        projection_revision: response.projection.projection_revision,
        child_projection_epoch: 0,
    }
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;
    use crate::{
        acp::{
            contract::{
                ClientIntentId, ProjectGeneration, RequiredText, RuntimeEpoch, SchemaVersionV1,
            },
            intent::{BindingId, CapabilitySnapshotV1, ExternalId, Prompt},
            store::{
                ActivateSessionCommand, CompleteActivationCommand, PrepareTurnCommand,
                StartSessionCommand, activate_session, complete_activation, start_session,
            },
        },
        persistence::Database,
    };

    #[test]
    fn disposition_refresh_releases_the_database_lock_and_advances_the_projection() {
        let root = std::env::temp_dir().join(format!("sage-edit-disposition-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let database = Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let chat_id = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let state = AppState::new(database);
        let project_id = SageProjectId::parse(&project.id).unwrap();
        let chat_id = SageChatId::parse(&chat_id).unwrap();
        let expected_projection_revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let session = {
            let mut connection = state.database.connection();
            start_session(
                &mut connection,
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision,
                    client_intent_id: ClientIntentId::parse("edit-disposition-start").unwrap(),
                    binding_id: BindingId::new("binding", "edit-disposition").unwrap(),
                    binding_digest: "a".repeat(64),
                    harness_version: None,
                    model_id: None,
                    now_millis: 1,
                },
            )
            .unwrap()
        };
        let revision = EntityRevision::new(9_007_199_254_740_993).unwrap();
        let epoch = RuntimeEpoch::new(1).unwrap();
        let page_revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        {
            let mut connection = state.database.connection();
            activate_session(
                &mut connection,
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: page_revision,
                    client_intent_id: ClientIntentId::parse("edit-disposition-activate").unwrap(),
                    session_id: session.session_id.clone(),
                    runtime_epoch: epoch,
                    artifact_digest: "a".repeat(64),
                    now_millis: 2,
                },
            )
            .unwrap();
            complete_activation(
                &mut connection,
                CompleteActivationCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session.session_id.clone(),
                    runtime_epoch: epoch,
                    binding_digest: "a".repeat(64),
                    artifact_digest: "a".repeat(64),
                    external_session_id: ExternalId::new("session", "disposition-refresh").unwrap(),
                    capabilities_json: serde_json::to_string(&CapabilitySnapshotV1 {
                        schema_version: SchemaVersionV1,
                        protocol_version: RequiredText::new("protocol", "fixture-v1").unwrap(),
                        new_session: true,
                        resume_session: false,
                        load_session: false,
                        close_session: true,
                        prompt: true,
                        cancel: true,
                        native_session_fork: false,
                        native_session_compact: false,
                        native_session_import: false,
                        native_review: false,
                        native_background_terminals: false,
                        active_turn_steer: false,
                        child_conversation_projection: false,
                        child_transcript_read: false,
                        collaboration_event_projection: false,
                        multi_root_extension_schema_id: None,
                        model_count: 1,
                        prompt_resources: false,
                        prompt_images: false,
                        config_count: 0,
                    })
                    .unwrap(),
                    now_millis: 3,
                },
            )
            .unwrap();
        }
        let page_revision = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let prepared = state
            .database
            .acp_prepare_turn(PrepareTurnCommand {
                project_id: project_id.clone(),
                project_generation: ProjectGeneration::ZERO,
                chat_id: chat_id.clone(),
                session_id: session.session_id.clone(),
                runtime_epoch: epoch,
                expected_projection_revision: page_revision,
                client_intent_id: ClientIntentId::parse("edit-disposition-turn").unwrap(),
                prompt: Prompt::new("prompt", "disposition refresh").unwrap(),
                now_millis: 4,
            })
            .unwrap();
        {
            let connection = state.database.connection();
            connection
                .execute(
                    "UPDATE turns SET revision=?1 WHERE id=?2",
                    params![revision.get() as i64, prepared.turn_id.as_str()],
                )
                .unwrap();
        }
        let before = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;

        let response = record_agent_edit_disposition_inner(
            &state,
            RecordAgentEditDispositionRequest {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                session_id: session.session_id.clone(),
                turn_id: prepared.turn_id,
                expected_revision: revision,
                state: AgentEditDispositionState::Accepted,
            },
        )
        .unwrap();

        assert!(response.projection.projection_revision > before);
        let refreshed = state
            .acp
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        assert_eq!(
            refreshed.projection_revision,
            response.projection.projection_revision
        );
        let event = projection_changed(project_id, chat_id, session.session_id, &response);
        assert_eq!(event.projection_revision, refreshed.projection_revision);

        fs::remove_dir_all(root).unwrap();
    }
}
