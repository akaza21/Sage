use std::sync::{Arc, Mutex};

use crate::acp::{
    projection::ActivityRole,
    store::{
        AcpStore, ActivateSessionCommand, ActivityIngressCommand, CloseSessionCommand,
        CompleteActivationCommand, FailActivationCommand, FinalizeCloseCommand, ItemIngressCommand,
        RequestStateCommand, StartSessionCommand, StoreCommit, StoreError, StoreMutation,
        TurnMutationCommand,
    },
};
use sage_acp_protocol::{
    contract::{
        ClientIntentId, EntityRevision, ProjectionRevision, RequiredText, RuntimeEpoch, SafeText,
        SageAgentSessionId, SageChatId, SageItemId, SageProjectId, SageServerRequestId, SageTurnId,
        SchemaVersionV1,
    },
    intent::{
        ActivityProvenanceV1, ActivitySourceKind, BindingId, ExternalId, ModelId, NormalizedPayload,
    },
    lifecycle::{
        ActivityLifecycle, DeliveryState, ItemLifecycle, ServerRequestState, SessionState,
        TurnState,
    },
    permissions::PermissionIntent,
};

use super::{JournalHandle, JournalLane};

struct RecordingStore(Arc<Mutex<Vec<&'static str>>>);

impl AcpStore for RecordingStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let [mutation] = batch else {
            return Err(StoreError::BatchShape);
        };
        self.0.lock().unwrap().push(kind(mutation));
        Ok(StoreCommit {
            projection_revision: ProjectionRevision::ZERO,
            results: Vec::new(),
        })
    }
}

#[test]
fn every_standalone_durable_family_crosses_the_single_journal_actor() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let committed = Arc::new(Mutex::new(Vec::new()));
        let (handle, runner) = JournalHandle::new(RecordingStore(Arc::clone(&committed)));
        let tickets: Vec<_> = admitted_mutations()
            .into_iter()
            .map(|mutation| handle.enqueue_one(JournalLane::Data, mutation, 0).unwrap())
            .collect();
        handle.close_when_drained();
        runner.spawn().await.unwrap();
        for ticket in tickets {
            ticket.committed().await.unwrap();
        }
        assert_eq!(
            *committed.lock().unwrap(),
            [
                "startSession",
                "activateSession",
                "completeActivation",
                "failActivation",
                "turnTerminal",
                "closeSession",
                "finalizeClose",
                "receivePermission",
                "requestState",
                "itemIngress",
                "activityIngress",
                "recoverAfterRestart",
            ]
        );
    });
}

fn admitted_mutations() -> Vec<StoreMutation> {
    let project = SageProjectId::parse("project-one").unwrap();
    let chat = SageChatId::parse("chat-one").unwrap();
    let session = SageAgentSessionId::parse("session-one").unwrap();
    let turn = SageTurnId::parse("turn-one").unwrap();
    let epoch = RuntimeEpoch::new(1).unwrap();
    let close = CloseSessionCommand {
        project_id: project.clone(),
        chat_id: chat.clone(),
        expected_projection_revision: ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::parse("close-one").unwrap(),
        session_id: session.clone(),
        runtime_epoch: epoch,
        now_millis: 1,
    };
    let permission = permission(&project, &chat, &session, &turn, epoch);
    let item = item(&project, &chat, &session, &turn, epoch);
    vec![
        StoreMutation::StartSession(StartSessionCommand {
            project_id: project.clone(),
            chat_id: chat.clone(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("start-one").unwrap(),
            binding_id: BindingId::new("binding", "fixture").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: Some(SafeText::new("version", "1.0.0").unwrap()),
            model_id: Some(ModelId::new("model", "fixture").unwrap()),
            now_millis: 1,
        }),
        StoreMutation::ActivateSession(ActivateSessionCommand {
            project_id: project.clone(),
            chat_id: chat.clone(),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("activate-one").unwrap(),
            session_id: session.clone(),
            runtime_epoch: epoch,
            artifact_digest: "b".repeat(64),
            now_millis: 1,
        }),
        StoreMutation::CompleteActivation(CompleteActivationCommand {
            project_id: project.clone(),
            chat_id: chat.clone(),
            session_id: session.clone(),
            runtime_epoch: epoch,
            binding_digest: "a".repeat(64),
            artifact_digest: "b".repeat(64),
            external_session_id: ExternalId::new("external", "external-session").unwrap(),
            capabilities_json: "{}".into(),
            now_millis: 1,
        }),
        StoreMutation::FailActivation(FailActivationCommand {
            project_id: project.clone(),
            chat_id: chat.clone(),
            session_id: session.clone(),
            runtime_epoch: epoch,
            binding_digest: "a".repeat(64),
            artifact_digest: "b".repeat(64),
            terminal: SessionState::Failed,
            exit_kind: RequiredText::new("exit", "fixture").unwrap(),
            now_millis: 1,
        }),
        StoreMutation::TurnTerminal {
            command: turn_command(&project, &chat, &session, &turn, epoch),
            terminal: TurnState::Cancelled,
            delivery: DeliveryState::NotWritten,
        },
        StoreMutation::CloseSession(close.clone()),
        StoreMutation::FinalizeClose(FinalizeCloseCommand { request: close }),
        StoreMutation::ReceivePermission {
            intent: permission.clone(),
            now_millis: 1,
        },
        StoreMutation::RequestState(RequestStateCommand {
            project_id: project.clone(),
            chat_id: chat.clone(),
            session_id: session.clone(),
            turn_id: turn.clone(),
            runtime_epoch: epoch,
            request_id: permission.request_id,
            state: ServerRequestState::DeclinePending,
            now_millis: 1,
        }),
        StoreMutation::ItemIngress(item.clone()),
        StoreMutation::ActivityIngress(activity(&project, &chat, &session, &turn, epoch)),
        StoreMutation::RecoverAfterRestart { now_millis: 1 },
    ]
}

fn permission(
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    turn: &SageTurnId,
    epoch: RuntimeEpoch,
) -> PermissionIntent {
    PermissionIntent {
        request_id: SageServerRequestId::parse("request-one").unwrap(),
        project_id: project.clone(),
        chat_id: chat.clone(),
        session_id: session.clone(),
        turn_id: turn.clone(),
        runtime_epoch: epoch,
        binding_digest: "a".repeat(64),
        external_request_id: ExternalId::new("external", "request-one").unwrap(),
        kind: RequiredText::new("kind", "permission").unwrap(),
        payload_digest: "c".repeat(64),
        normalized_payload_digest: "d".repeat(64),
        normalized_payload_json: "{}".into(),
    }
}

fn item(
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    turn: &SageTurnId,
    epoch: RuntimeEpoch,
) -> ItemIngressCommand {
    ItemIngressCommand {
        project_id: project.clone(),
        chat_id: chat.clone(),
        session_id: session.clone(),
        turn_id: turn.clone(),
        runtime_epoch: epoch,
        external_item_id: ExternalId::new("external", "item-one").unwrap(),
        activity_id: None,
        payload: NormalizedPayload::Text {
            text: SafeText::new("text", "hello").unwrap(),
        },
        payload_digest: "d".repeat(64),
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        now_millis: 1,
    }
}

fn activity(
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    turn: &SageTurnId,
    epoch: RuntimeEpoch,
) -> ActivityIngressCommand {
    ActivityIngressCommand {
        project_id: project.clone(),
        chat_id: chat.clone(),
        session_id: session.clone(),
        turn_id: turn.clone(),
        runtime_epoch: epoch,
        external_activity_id: ExternalId::new("external", "activity-one").unwrap(),
        activity_kind: RequiredText::new("kind", "subagent").unwrap(),
        parent_activity_id: None,
        parent_item_id: SageItemId::parse("item-one").unwrap(),
        role: ActivityRole::Child,
        lifecycle: ActivityLifecycle::Observed,
        depth: 1,
        observed_sequence: 1,
        provenance: ActivityProvenanceV1 {
            schema_version: SchemaVersionV1,
            source_kind: ActivitySourceKind::Collaboration,
            external_kind: RequiredText::new("kind", "subagent").unwrap(),
            extension_schema_id: None,
            safe_label: None,
        },
        provenance_digest: "e".repeat(64),
        now_millis: 1,
    }
}

fn turn_command(
    project: &SageProjectId,
    chat: &SageChatId,
    session: &SageAgentSessionId,
    turn: &SageTurnId,
    epoch: RuntimeEpoch,
) -> TurnMutationCommand {
    TurnMutationCommand {
        project_id: project.clone(),
        chat_id: chat.clone(),
        session_id: session.clone(),
        runtime_epoch: epoch,
        turn_id: turn.clone(),
        expected_revision: EntityRevision::ZERO,
        now_millis: 1,
    }
}

fn kind(mutation: &StoreMutation) -> &'static str {
    match mutation {
        StoreMutation::StartSession(_) => "startSession",
        StoreMutation::ActivateSession(_) => "activateSession",
        StoreMutation::CompleteActivation(_) => "completeActivation",
        StoreMutation::FailActivation(_) => "failActivation",
        StoreMutation::TurnTerminal { .. } => "turnTerminal",
        StoreMutation::CloseSession(_) => "closeSession",
        StoreMutation::FinalizeClose(_) => "finalizeClose",
        StoreMutation::ReceivePermission { .. } => "receivePermission",
        StoreMutation::RequestState(_) => "requestState",
        StoreMutation::ItemIngress(_) => "itemIngress",
        StoreMutation::ActivityIngress(_) => "activityIngress",
        StoreMutation::AdapterInboundBatch(_) => "adapterInboundBatch",
        StoreMutation::RecoverAfterRestart { .. } => "recoverAfterRestart",
        StoreMutation::RecoverRuntimeAfterZero(_) => "recoverRuntimeAfterZero",
        _ => "legacy",
    }
}
