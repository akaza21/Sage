use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};

use crate::acp::{
    projection::ActivityRole,
    store::{
        AcpStore, AdapterActivityItemCommand, AdapterBatchMutation, AdapterTurnCursor,
        EntityMutation, MutationDisposition, RequestStateCommand, StoreCommit, StoreError,
        StoreMutation,
    },
};
use sage_acp_protocol::{
    contract::{
        RequiredText, RuntimeEpoch, SafeText, SageAgentSessionId, SageChatId, SageProjectId,
        SageServerRequestId, SageTurnId, SchemaVersionV1,
    },
    intent::{ActivityProvenanceV1, ActivitySourceKind, ExternalId, NormalizedPayload},
    lifecycle::{ActivityLifecycle, ItemLifecycle, ServerRequestState, TurnState},
};

use super::*;
use crate::acp::runtime::adapter_pump::{ActiveTurnIntent, DurableActivityItem, SessionPumpIntent};
use sage_acp_protocol::permissions::PermissionIntent;

struct RecordingStore(Arc<Mutex<usize>>);
struct RequestStateStore(Arc<Mutex<Option<RequestStateCommand>>>);

struct Clock(AtomicI64);

impl DriverClock for Clock {
    fn now_millis(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

impl AcpStore for RecordingStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let [StoreMutation::AdapterInboundBatch(_)] = batch else {
            return Err(StoreError::BatchShape);
        };
        *self.0.lock().unwrap() += 1;
        let revision = sage_acp_protocol::contract::ProjectionRevision::new(9).unwrap();
        Ok(StoreCommit {
            projection_revision: revision,
            results: vec![StoreMutationResult::AdapterBatch(
                AdapterBatchMutation::test_only(
                    revision,
                    MutationDisposition::Applied,
                    AdapterTurnCursor::test_only(
                        SageTurnId::parse("turn-one").unwrap(),
                        TurnState::Running,
                        sage_acp_protocol::lifecycle::DeliveryState::Correlated,
                        sage_acp_protocol::contract::EntityRevision::new(2).unwrap(),
                        revision,
                    ),
                ),
            )],
        })
    }
}

impl AcpStore for RequestStateStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let [StoreMutation::RequestState(command)] = batch else {
            return Err(StoreError::BatchShape);
        };
        *self.0.lock().unwrap() = Some(command.clone());
        let projection_revision = sage_acp_protocol::contract::ProjectionRevision::new(10).unwrap();
        Ok(StoreCommit {
            projection_revision,
            results: vec![StoreMutationResult::Entity(EntityMutation {
                revision: sage_acp_protocol::contract::EntityRevision::new(2).unwrap(),
                projection_revision,
                disposition: MutationDisposition::Applied,
            })],
        })
    }
}

#[test]
fn journal_handle_commits_one_store_command_and_returns_the_final_revision() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let calls = Arc::new(Mutex::new(0));
        let (handle, runner) = JournalHandle::new(RecordingStore(Arc::clone(&calls)));
        let task = runner.spawn();
        let mut journal = SqliteAdapterBatchJournal::new(handle.clone(), intent());
        let receipt = journal
            .commit(vec![turn_event(TurnState::Running)], 128)
            .await
            .unwrap();
        handle.close_when_drained();
        task.await.unwrap();
        assert_eq!(receipt.projection_revision.get(), 9);
        assert_eq!(receipt.turn_cursor().turn_id().as_str(), "turn-one");
        assert_eq!(receipt.turn_cursor().state(), TurnState::Running);
        assert_eq!(*calls.lock().unwrap(), 1);
    });
}

#[test]
fn every_adapter_commit_gets_a_fresh_bounded_deadline_after_rebind() {
    let (handle, _runner) = JournalHandle::new(RecordingStore(Arc::new(Mutex::new(0))));
    let clock = Arc::new(Clock(AtomicI64::new(1_000)));
    let first = SqliteAdapterBatchJournal::with_clock(handle, intent(), clock.clone());
    assert_eq!(first.next_deadline(), 31_000);

    clock.0.store(61_000, Ordering::SeqCst);
    let second = match first.try_rebind(intent()) {
        Ok(second) => second,
        Err(_) => panic!("rebind"),
    };
    assert_eq!(second.next_deadline(), 91_000);
}

#[test]
fn permission_transition_commits_one_standalone_request_state() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let observed = Arc::new(Mutex::new(None));
        let (handle, runner) = JournalHandle::new(RequestStateStore(Arc::clone(&observed)));
        let task = runner.spawn();
        let mut journal = SqliteAdapterBatchJournal::new(handle.clone(), intent());
        let permission = permission_intent();
        let revision = journal
            .transition_permission(&permission, ServerRequestState::ResponseQueued, 22)
            .await
            .unwrap();
        handle.close_when_drained();
        task.await.unwrap();

        assert_eq!(revision.get(), 10);
        let command = observed.lock().unwrap().clone().unwrap();
        assert_eq!(command.request_id, permission.request_id);
        assert_eq!(command.state, ServerRequestState::ResponseQueued);
        assert_eq!(command.now_millis, 22);
    });
}

#[test]
fn count_limit_and_foreign_scope_reject_before_journal_allocation() {
    let intent = intent();
    let over = (0..=LIMITS.queues.journal_facts_per_transaction)
        .map(|_| turn_event(TurnState::Running))
        .collect();
    assert!(matches!(
        build_command(&intent, over, 128),
        Err(PumpJournalError::Rejected)
    ));
    assert!(matches!(
        build_command(
            &intent,
            vec![turn_event(TurnState::Running)],
            LIMITS.queues.journal_bytes_per_transaction as usize + 1
        ),
        Err(PumpJournalError::Rejected)
    ));
    let mut foreign = turn_event(TurnState::Running);
    let DurableAdapterEvent::Turn { scope, .. } = &mut foreign else {
        unreachable!()
    };
    scope.binding_digest = "f".repeat(64);
    assert!(matches!(
        build_command(&intent, vec![foreign], 128),
        Err(PumpJournalError::Rejected)
    ));
}

#[test]
fn boxed_activity_retained_bytes_charge_the_pointee_and_payload_plus_one() {
    let activity = activity_event("bounded activity");
    let payload_bytes = serde_json::to_vec(&activity.payload).unwrap().len();
    let provenance_bytes = serde_json::to_vec(&activity.provenance).unwrap().len();
    let expected = std::mem::size_of::<AdapterActivityItemCommand>()
        + scope_heap_bytes(&activity.scope)
        + activity.external_activity_id.as_str().len()
        + activity.external_item_id.as_str().len()
        + activity.parent_external_item_id.as_str().len()
        + activity.activity_kind.as_str().len()
        + provenance_bytes
        + activity.payload_digest.len()
        + payload_bytes;
    assert_eq!(
        activity::event_bytes(&intent(), &activity).unwrap(),
        expected
    );

    let plus_one = activity_event("bounded activity+");
    assert_eq!(
        activity::event_bytes(&intent(), &plus_one).unwrap(),
        expected + 1
    );
}

fn intent() -> PumpIntent {
    SessionPumpIntent {
        binding_digest: [7; 32],
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        external_session_id: ExternalId::new("external", "session-external").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    }
    .bind_turn(ActiveTurnIntent {
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        external_turn_id: Some(ExternalId::new("external", "turn-external").unwrap()),
        client_message_id: RequiredText::new("message", "message-one").unwrap(),
    })
}

fn turn_event(state: TurnState) -> DurableAdapterEvent {
    let intent = intent();
    DurableAdapterEvent::Turn {
        scope: TurnScope {
            project_id: intent.project_id,
            chat_id: intent.chat_id,
            session_id: intent.session_id,
            runtime_epoch: intent.runtime_epoch,
            turn_id: intent.active_turn.turn_id,
            binding_digest: "07".repeat(32),
        },
        state,
        now_millis: 1,
    }
}

fn activity_event(text: &str) -> DurableActivityItem {
    let intent = intent();
    let payload = NormalizedPayload::Text {
        text: SafeText::new("activityText", text).unwrap(),
    };
    let payload_digest = digest(&serde_json::to_vec(&payload).unwrap());
    DurableActivityItem {
        scope: TurnScope {
            project_id: intent.project_id,
            chat_id: intent.chat_id,
            session_id: intent.session_id,
            runtime_epoch: intent.runtime_epoch,
            turn_id: intent.active_turn.turn_id,
            binding_digest: "07".repeat(32),
        },
        external_activity_id: ExternalId::new("activity", "activity-one").unwrap(),
        external_item_id: ExternalId::new("item", "item-one").unwrap(),
        parent_external_activity_id: None,
        parent_external_item_id: ExternalId::new("item", "parent-one").unwrap(),
        activity_kind: RequiredText::new("activityKind", "nativeChild").unwrap(),
        role: ActivityRole::Child,
        activity_lifecycle: ActivityLifecycle::Completed,
        depth: 1,
        observed_sequence: 1,
        provenance: ActivityProvenanceV1 {
            schema_version: SchemaVersionV1,
            source_kind: ActivitySourceKind::Tool,
            external_kind: RequiredText::new("externalKind", "fixture").unwrap(),
            extension_schema_id: None,
            safe_label: Some(RequiredText::new("safeLabel", "activity").unwrap()),
        },
        item_lifecycle: ItemLifecycle::Completed,
        item_tool_state: None,
        payload,
        payload_digest,
        now_millis: 1,
    }
}

fn permission_intent() -> PermissionIntent {
    let owner = intent();
    PermissionIntent {
        request_id: SageServerRequestId::parse("request-one").unwrap(),
        project_id: owner.project_id,
        chat_id: owner.chat_id,
        session_id: owner.session_id,
        turn_id: owner.active_turn.turn_id,
        runtime_epoch: owner.runtime_epoch,
        binding_digest: "07".repeat(32),
        external_request_id: ExternalId::new("request", "external-request").unwrap(),
        kind: RequiredText::new("kind", "permission").unwrap(),
        payload_digest: "11".repeat(32),
        normalized_payload_digest: "22".repeat(32),
        normalized_payload_json: "{}".to_owned(),
    }
}
