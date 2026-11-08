use super::*;
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Instant,
};

use crate::{
    acp::{
        contract::{RequiredText, SageTurnId, SchemaVersionV1},
        goal::{GoalSnapshot, GoalState, GoalStatus, NativeRpcIntent, NativeRpcReply},
        intent::{BindingId, CapabilitySnapshotV1, ExternalId},
        runtime::{
            HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
            NativeRpcCompletionSink, PromptWrite, RuntimeEngine, RuntimeHost,
        },
        store::{
            ActivateSessionCommand, AdapterTurnCursor, CompleteActivationCommand,
            StartSessionCommand, activate_session, complete_activation, start_session,
        },
    },
    persistence::Database,
};

struct GoalEngine {
    result: Result<(), HostError>,
    calls: Arc<AtomicUsize>,
    channel: Arc<crate::acp::goal::GoalChannel>,
}

impl RuntimeEngine for GoalEngine {
    fn begin_native_rpc(
        &mut self,
        intent: &NativeRpcIntent,
        _: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.result.is_ok() {
            let next = match &intent.request {
                crate::acp::goal::NativeRpcRequest::Goal(GoalAction::PauseExisting) => {
                    GoalState::Goal {
                        snapshot: GoalSnapshot {
                            objective: "Keep the native goal".into(),
                            status: GoalStatus::Paused,
                            token_budget: None,
                            tokens_used: 3,
                            time_used_seconds: 6,
                            created_at: None,
                            updated_at: None,
                        },
                    }
                }
                crate::acp::goal::NativeRpcRequest::Goal(GoalAction::ClearExisting) => {
                    GoalState::NoGoal
                }
                crate::acp::goal::NativeRpcRequest::ChildThreadRead { .. }
                | crate::acp::goal::NativeRpcRequest::ForkSession
                | crate::acp::goal::NativeRpcRequest::ReleaseFork { .. } => {
                    return Err(HostError::Intent);
                }
            };
            self.channel.update(&intent.owner, next);
        }
        completion.complete(self.result.clone().map(|()| NativeRpcReply::GoalApplied));
        Ok(())
    }
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

fn fixture(result: Result<(), HostError>) -> (AppState, ExistingGoalRequest, Arc<AtomicUsize>) {
    let root = std::env::temp_dir().join(format!("sage-goal-ipc-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(Path::new(":memory:")).unwrap();
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
    let session = {
        let mut connection = state.database.connection();
        start_session(
            &mut connection,
            StartSessionCommand {
                project_id: project_id.clone(),
                chat_id: chat_id.clone(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("goal-start").unwrap(),
                binding_id: BindingId::new("binding", "goal-test").unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
        )
        .unwrap()
    };
    let epoch = RuntimeEpoch::new(1).unwrap();
    let revision = state
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
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("goal-activate").unwrap(),
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
                external_session_id: ExternalId::new("session", "goal-native").unwrap(),
                capabilities_json: serde_json::to_string(&CapabilitySnapshotV1 {
                    schema_version: SchemaVersionV1,
                    protocol_version: RequiredText::new(
                        "protocol",
                        crate::acp::bindings::codex::CODEX_OBSERVED_PROTOCOL_IDENTITY,
                    )
                    .unwrap(),
                    new_session: true,
                    resume_session: false,
                    load_session: false,
                    close_session: true,
                    prompt: true,
                    cancel: true,
                    native_session_fork: false,
                    active_turn_steer: false,
                    child_conversation_projection: false,
                    child_transcript_read: false,
                    collaboration_event_projection: false,
                    multi_root_extension_schema_id: None,
                    model_count: 1,
                    prompt_resources: false,
                    config_count: 0,
                })
                .unwrap(),
                now_millis: 3,
            },
        )
        .unwrap();
    }
    let owner = RuntimeOwner {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: session.session_id.clone(),
        runtime_epoch: epoch,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let host = Arc::new(
        RuntimeHost::start(GoalEngine {
            result,
            calls: Arc::clone(&calls),
            channel: state.acp.goal_channel_for_test(),
        })
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    state.acp.install_goal_test_runtime(
        owner,
        host,
        GoalState::Goal {
            snapshot: GoalSnapshot {
                objective: "Keep the native goal".into(),
                status: GoalStatus::Active,
                token_budget: None,
                tokens_used: 3,
                time_used_seconds: 5,
                created_at: None,
                updated_at: None,
            },
        },
    );
    let revision = state
        .acp
        .load_chat_page(project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    (
        state,
        ExistingGoalRequest {
            project_id,
            project_generation: ProjectGeneration::ZERO,
            chat_id,
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::parse("goal-action").unwrap(),
            session_id: session.session_id,
            runtime_epoch: epoch,
        },
        calls,
    )
}

#[tokio::test]
async fn sqlite_head_and_epoch_gate_goal_ipc_before_host_dispatch() {
    let (state, request, calls) = fixture(Ok(()));
    let mut stale = ExistingGoalRequest {
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        ..request
    };
    assert!(
        mutate(&state, stale.clone(), GoalAction::PauseExisting)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    stale = ExistingGoalRequest {
        session_id: SageAgentSessionId::parse("foreign-session").unwrap(),
        ..stale
    };
    assert!(
        mutate(&state, stale, GoalAction::ClearExisting)
            .await
            .is_err()
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    state.acp.shutdown_blocking().unwrap();
}

#[tokio::test]
async fn sqlite_owned_goal_ipc_reports_correlated_result_and_never_replays_unknown() {
    for (result, rejected) in [
        (Ok(()), false),
        (Err(HostError::GoalMutationRejected), true),
        (Err(HostError::DeliveryUnknown), true),
    ] {
        let unknown = result == Err(HostError::DeliveryUnknown);
        let (state, request, calls) = fixture(result);
        let outcome = mutate(&state, request.clone(), GoalAction::PauseExisting).await;
        assert_eq!(outcome.is_err(), rejected);
        if let Ok(response) = &outcome {
            assert!(
                matches!(&response.projection.goal, GoalState::Goal { snapshot } if snapshot.status == GoalStatus::Paused)
            );
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        if unknown {
            assert!(
                mutate(&state, request, GoalAction::PauseExisting)
                    .await
                    .is_err()
            );
            assert_eq!(calls.load(Ordering::SeqCst), 1);
        }
        state.acp.shutdown_blocking().unwrap();
    }
}

#[tokio::test]
async fn sqlite_owned_clear_ipc_returns_a_no_goal_projection() {
    let (state, request, calls) = fixture(Ok(()));
    let response = mutate(&state, request, GoalAction::ClearExisting)
        .await
        .unwrap();
    assert_eq!(response.projection.goal, GoalState::NoGoal);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    state.acp.shutdown_blocking().unwrap();
}
