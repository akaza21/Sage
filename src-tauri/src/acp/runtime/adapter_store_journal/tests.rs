use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, Ordering},
};

use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageTurnId,
    },
    intent::ExternalId,
    lifecycle::TurnState,
    store::{
        AcpStore, AdapterBatchMutation, AdapterTurnCursor, MutationDisposition, StoreCommit,
        StoreError, StoreMutation,
    },
};

use super::*;
use crate::acp::runtime::adapter_pump::{ActiveTurnIntent, SessionPumpIntent};

struct RecordingStore(Arc<Mutex<usize>>);

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
        let revision = crate::acp::contract::ProjectionRevision::new(9).unwrap();
        Ok(StoreCommit {
            projection_revision: revision,
            results: vec![StoreMutationResult::AdapterBatch(
                AdapterBatchMutation::test_only(
                    revision,
                    MutationDisposition::Applied,
                    AdapterTurnCursor::test_only(
                        SageTurnId::parse("turn-one").unwrap(),
                        TurnState::Running,
                        crate::acp::lifecycle::DeliveryState::Correlated,
                        crate::acp::contract::EntityRevision::new(2).unwrap(),
                        revision,
                    ),
                ),
            )],
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
