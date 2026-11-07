use std::sync::{Arc, Mutex};

use super::*;
use crate::acp::{
    contract::{
        ClientIntentId, EntityRevision, ProjectGeneration, ProjectionRevision, RuntimeEpoch,
        SageAgentSessionId, SageChatId, SageItemId, SageProjectId, SageTurnId,
    },
    intent::{IntentSnapshot, Prompt},
    scheduler::{RuntimeOwner, Scheduler},
    store::{
        AcpStore, AdapterTurnCursor, MutationDisposition, PrepareTurnCommand, PreparedTurn,
        SendObservation, StoreCommit, StoreError, StoreMutation, StoreMutationResult, TurnMutation,
        TurnMutationCommand,
    },
};

#[derive(Clone)]
struct Authority(IntentSnapshot);
impl ProjectIntentAuthority for Authority {
    fn current_snapshot(&self, _: &PrepareTurnCommand) -> Option<IntentSnapshot> {
        Some(self.0.clone())
    }
}

struct FakeStore {
    order: Arc<Mutex<Vec<String>>>,
    duplicate: Option<PreparedTurn>,
    fail_prepare: bool,
    cancel: Option<(ClientIntentId, TurnMutation)>,
}

impl AcpStore for FakeStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let [mutation] = batch else {
            return Err(StoreError::BatchShape);
        };
        match mutation {
            StoreMutation::LookupTurn(_) => Ok(StoreCommit {
                projection_revision: self
                    .duplicate
                    .as_ref()
                    .map_or(ProjectionRevision::ZERO, |value| value.projection_revision),
                results: vec![StoreMutationResult::Lookup(self.duplicate.clone())],
            }),
            StoreMutation::PrepareTurn(command) => {
                self.order
                    .lock()
                    .unwrap()
                    .push(command.client_intent_id.as_str().into());
                if self.fail_prepare {
                    return Err(StoreError::Capacity {
                        resource: "history",
                        limit: 1,
                    });
                }
                let prepared = prepared(1);
                Ok(StoreCommit {
                    projection_revision: prepared.projection_revision,
                    results: vec![StoreMutationResult::Prepared(prepared)],
                })
            }
            StoreMutation::BeginSend(command) => {
                self.order
                    .lock()
                    .unwrap()
                    .push(command.turn_id.as_str().into());
                let turn = TurnMutation {
                    state: crate::acp::lifecycle::TurnState::Sending,
                    delivery_state: crate::acp::lifecycle::DeliveryState::Writing,
                    revision: command.expected_revision.next().unwrap(),
                    projection_revision: ProjectionRevision::new(2).unwrap(),
                    disposition: MutationDisposition::Applied,
                };
                Ok(StoreCommit {
                    projection_revision: turn.projection_revision,
                    results: vec![StoreMutationResult::Turn(turn)],
                })
            }
            StoreMutation::ObserveSend {
                command,
                observation,
                ..
            } => {
                let terminal = matches!(
                    observation,
                    SendObservation::PossibleBytes | SendObservation::ProcessLost
                );
                let turn = TurnMutation {
                    state: if terminal {
                        crate::acp::lifecycle::TurnState::DeliveryUnknown
                    } else {
                        crate::acp::lifecycle::TurnState::Running
                    },
                    delivery_state: if terminal {
                        crate::acp::lifecycle::DeliveryState::PossibleBytes
                    } else {
                        crate::acp::lifecycle::DeliveryState::Correlated
                    },
                    revision: command.expected_revision.next().unwrap(),
                    projection_revision: ProjectionRevision::new(3).unwrap(),
                    disposition: MutationDisposition::Applied,
                };
                Ok(StoreCommit {
                    projection_revision: turn.projection_revision,
                    results: vec![StoreMutationResult::Turn(turn)],
                })
            }
            StoreMutation::BeginCancel(command) => {
                self.order
                    .lock()
                    .unwrap()
                    .push(format!("cancel:{}", command.client_intent_id.as_str()));
                if let Some((intent, mutation)) = self.cancel.as_ref() {
                    if intent != &command.client_intent_id {
                        return Err(StoreError::Conflict {
                            resource: "cancelIntent",
                        });
                    }
                    let mut duplicate = mutation.clone();
                    duplicate.disposition = MutationDisposition::Duplicate;
                    return Ok(StoreCommit {
                        projection_revision: duplicate.projection_revision,
                        results: vec![StoreMutationResult::Turn(duplicate)],
                    });
                }
                let mutation = TurnMutation {
                    state: crate::acp::lifecycle::TurnState::Cancelling,
                    delivery_state: crate::acp::lifecycle::DeliveryState::Correlated,
                    revision: command.turn.expected_revision.next().unwrap(),
                    projection_revision: ProjectionRevision::new(4).unwrap(),
                    disposition: MutationDisposition::Applied,
                };
                self.cancel = Some((command.client_intent_id.clone(), mutation.clone()));
                Ok(StoreCommit {
                    projection_revision: mutation.projection_revision,
                    results: vec![StoreMutationResult::Turn(mutation)],
                })
            }
            _ => Err(StoreError::BatchShape),
        }
    }
}

fn command() -> PrepareTurnCommand {
    PrepareTurnCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        project_generation: ProjectGeneration::new(4).unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
        expected_projection_revision: ProjectionRevision::ZERO,
        client_intent_id: ClientIntentId::parse("intent-one").unwrap(),
        prompt: Prompt::new("prompt", "hello").unwrap(),
        now_millis: super::journal::now_millis() as i64,
    }
}

fn snapshot(turn_id: Option<SageTurnId>) -> IntentSnapshot {
    let command = command();
    IntentSnapshot {
        project_id: command.project_id,
        project_generation: command.project_generation,
        chat_id: command.chat_id,
        projection_revision: command.expected_projection_revision,
        intent_revision: command.expected_projection_revision,
        session_id: Some(command.session_id),
        runtime_epoch: Some(command.runtime_epoch),
        turn_id,
        turn_state: None,
        turn_revision: None,
    }
}

fn prepared(revision: u64) -> PreparedTurn {
    PreparedTurn {
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        item_id: SageItemId::parse("item-one").unwrap(),
        turn_revision: EntityRevision::ZERO,
        projection_revision: ProjectionRevision::new(revision).unwrap(),
        disposition: MutationDisposition::Applied,
    }
}

fn begin_mutation(id: &str) -> StoreMutation {
    StoreMutation::BeginSend(TurnMutationCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse(id).unwrap(),
        expected_revision: EntityRevision::ZERO,
        now_millis: 0,
    })
}

fn fake(
    duplicate: Option<PreparedTurn>,
    fail_prepare: bool,
) -> (
    JournalHandle,
    JournalRunner<FakeStore>,
    Arc<Mutex<Vec<String>>>,
) {
    let order = Arc::new(Mutex::new(Vec::new()));
    let (handle, runner) = JournalHandle::new(FakeStore {
        order: Arc::clone(&order),
        duplicate,
        fail_prepare,
        cancel: None,
    });
    (handle, runner, order)
}

#[test]
fn data_saturation_preserves_the_eight_batch_control_reserve() {
    let (handle, _runner, _) = fake(None, false);
    let data: Vec<_> = (0..120)
        .map(|_| handle.batch(JournalLane::Data).unwrap())
        .collect();
    assert!(matches!(
        handle.batch(JournalLane::Data),
        Err(JournalError::Capacity { lane: "data" })
    ));
    let control: Vec<_> = (0..8)
        .map(|_| handle.batch(JournalLane::Control).unwrap())
        .collect();
    assert!(matches!(
        handle.batch(JournalLane::Control),
        Err(JournalError::Capacity { lane: "control" })
    ));
    drop((data, control));
}

#[test]
fn batch_reserves_before_push_and_rejects_fact_limit_plus_one() {
    let (handle, _runner, _) = fake(None, false);
    let mut batch = handle.batch(JournalLane::Data).unwrap();
    for _ in 0..128 {
        batch.push(begin_mutation("turn-one")).unwrap();
    }
    assert!(matches!(
        batch.push(begin_mutation("turn-over")),
        Err(JournalError::BatchLimit)
    ));
}

#[test]
fn retained_bytes_bound_data_lane_before_the_count_limit() {
    let (handle, _runner, _) = fake(None, false);
    let mut held = Vec::new();
    loop {
        let mut batch = handle.batch(JournalLane::Data).unwrap();
        let mut large = command();
        large.prompt = Prompt::new("prompt", "x".repeat(100_000)).unwrap();
        match batch.push(StoreMutation::PrepareTurn(large)) {
            Ok(()) => held.push(batch),
            Err(JournalError::Capacity { lane: "data" }) => break,
            result => panic!("unexpected byte-bound result: {result:?}"),
        }
    }
    assert!(held.len() < 120);
    assert!(held.len() >= 37);
}

#[test]
fn control_message_limit_rejects_before_retaining_oversized_mutation() {
    let (handle, _runner, _) = fake(None, false);
    let mut batch = handle.batch(JournalLane::Control).unwrap();
    let mut large = command();
    large.prompt = Prompt::new("prompt", "x".repeat(40_000)).unwrap();
    assert!(matches!(
        batch.push(StoreMutation::PrepareTurn(large)),
        Err(JournalError::BatchLimit)
    ));
    drop(batch);
    assert!(handle.batch(JournalLane::Control).is_ok());
}

#[test]
fn control_uses_earliest_deadline_then_fifo_before_data() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (handle, runner, order) = fake(None, false);
        let base = super::journal::now_millis();
        let data = handle
            .enqueue_one(JournalLane::Data, begin_mutation("data"), 0)
            .unwrap();
        let late = handle
            .enqueue_one(
                JournalLane::Control,
                begin_mutation("late"),
                base.saturating_add(30_000),
            )
            .unwrap();
        let early = handle
            .enqueue_one(
                JournalLane::Control,
                begin_mutation("early"),
                base.saturating_add(10_000),
            )
            .unwrap();
        handle.close_when_drained();
        runner.spawn().await.unwrap();
        let _ = (
            data.committed().await,
            late.committed().await,
            early.committed().await,
        );
        assert_eq!(&*order.lock().unwrap(), &["early", "late", "data"]);
    });
}

#[test]
fn expired_entry_never_reaches_the_store_and_releases_capacity() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (handle, runner, order) = fake(None, false);
        let ticket = handle
            .enqueue_one(JournalLane::Data, begin_mutation("expired"), 1)
            .unwrap();
        handle.close_when_drained();
        runner.spawn().await.unwrap();
        assert!(matches!(
            ticket.committed().await,
            Err(JournalError::Deadline)
        ));
        assert!(order.lock().unwrap().is_empty());
        assert_eq!(handle.retained_counts().0, 0);
    });
}

#[test]
fn queued_ticket_timeout_is_definitely_uncommitted_and_skipped_by_the_runner() {
    let (handle, runner, order) = fake(None, false);
    let ticket = handle
        .enqueue_one(
            JournalLane::Data,
            begin_mutation("queued-timeout"),
            super::journal::now_millis().saturating_add(20),
        )
        .unwrap();
    assert!(matches!(
        ticket.blocking_committed(),
        Err(JournalError::Deadline)
    ));
    handle.close_when_drained();
    runner.run_blocking();
    assert!(order.lock().unwrap().is_empty());
    assert_eq!(handle.retained_counts().0, 0);
}

struct BlockingCommitStore {
    gate: Arc<(std::sync::Condvar, Mutex<(bool, bool)>)>,
}

impl AcpStore for BlockingCommitStore {
    fn commit(&mut self, _: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let (ready, state) = &*self.gate;
        let mut state = state.lock().unwrap();
        state.0 = true;
        ready.notify_all();
        while !state.1 {
            let (next, timed) = ready
                .wait_timeout(state, crate::test_wait::HANG_GUARD)
                .unwrap();
            state = next;
            assert!(
                !timed.timed_out() || state.1,
                "the blocking commit store was never released by its test"
            );
        }
        Ok(StoreCommit {
            projection_revision: ProjectionRevision::new(1).unwrap(),
            results: Vec::new(),
        })
    }
}

#[test]
fn in_flight_ticket_timeout_is_commit_unknown_and_retains_capacity() {
    let gate = Arc::new((std::sync::Condvar::new(), Mutex::new((false, false))));
    let (handle, runner) = JournalHandle::new(BlockingCommitStore {
        gate: Arc::clone(&gate),
    });
    let mut ticket = handle
        .enqueue_one(
            JournalLane::Data,
            begin_mutation("in-flight-timeout"),
            super::journal::now_millis()
                .saturating_add(u64::try_from(crate::test_wait::BUDGET.as_millis()).unwrap()),
        )
        .unwrap();
    let thread = std::thread::spawn(move || runner.run_blocking());
    let (ready, state_lock) = &*gate;
    let mut state = state_lock.lock().unwrap();
    while !state.0 {
        let (next, timed) = ready
            .wait_timeout(state, crate::test_wait::HANG_GUARD)
            .unwrap();
        state = next;
        assert!(
            !timed.timed_out() || state.0,
            "the journal runner never entered the blocking commit store"
        );
    }
    drop(state);

    ticket.expire_deadline_for_test();
    assert!(matches!(
        ticket.blocking_committed(),
        Err(JournalError::Store(StoreError::CommitUnknown))
    ));
    let mut state = state_lock.lock().unwrap();
    state.1 = true;
    ready.notify_all();
    drop(state);
    handle.close_when_drained();
    thread.join().unwrap();
    assert_eq!(handle.retained_counts().0, 1);
    handle.release_unknown_reservations();
    assert_eq!(handle.retained_counts().0, 0);
}

struct CommitUnknownStore;
impl AcpStore for CommitUnknownStore {
    fn commit(&mut self, _: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        Err(StoreError::CommitUnknown)
    }
}

struct PrepareCommitUnknownStore;

impl AcpStore for PrepareCommitUnknownStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        match batch {
            [StoreMutation::LookupTurn(_)] => Ok(StoreCommit {
                projection_revision: ProjectionRevision::ZERO,
                results: vec![StoreMutationResult::Lookup(None)],
            }),
            [StoreMutation::PrepareTurn(_)] => Err(StoreError::CommitUnknown),
            _ => Err(StoreError::BatchShape),
        }
    }
}

#[test]
fn commit_unknown_retains_reservation_until_authoritative_recovery() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (handle, runner) = JournalHandle::new(CommitUnknownStore);
        let ticket = handle
            .enqueue_one(JournalLane::Data, begin_mutation("unknown"), 0)
            .unwrap();
        handle.close_when_drained();
        runner.spawn().await.unwrap();
        assert!(matches!(
            ticket.committed().await,
            Err(JournalError::Store(StoreError::CommitUnknown))
        ));
        assert_eq!(handle.retained_counts().0, 1);
        handle.release_unknown_reservations();
        assert_eq!(handle.retained_counts().0, 0);
    });
}

#[test]
fn prepare_commit_unknown_retains_the_exact_turn_reservation_until_lookup_resolution() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (handle, runner) = JournalHandle::new(PrepareCommitUnknownStore);
        let runner = runner.spawn();
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(snapshot(None))),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();

        let Err(mut failure) = coordinator.prepare(&tree, command()).await else {
            panic!("commit-unknown must retain recovery")
        };
        assert!(matches!(
            failure.error,
            CoordinatorError::Journal(JournalError::Store(StoreError::CommitUnknown))
        ));
        let recovery = failure.recovery.take().expect("exact reserved Turn");
        let ambiguous = recovery.ambiguous().expect("lookup required");
        assert_eq!(
            ambiguous.command().client_intent_id,
            command().client_intent_id
        );
        assert!(matches!(
            tree.reserve_turn(),
            Err(crate::acp::scheduler::SchedulerError::TurnCapacity)
        ));
        handle.close_when_drained();
        runner.await.unwrap();
        assert_eq!(handle.retained_counts().0, 1);
        let PrepareRecovery::Ambiguous(ambiguous) = recovery else {
            panic!("ambiguous recovery")
        };
        ambiguous.abort_proven_absent().unwrap();
        handle.release_unknown_reservations();
        assert_eq!(handle.retained_counts().0, 0);
        tree.reserve_turn()
            .unwrap()
            .abort_before_durable_commit()
            .unwrap();
    });
}

#[test]
fn post_commit_bootstrap_failure_retains_dispatch_and_terminal_release() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (handle, runner, _) = fake(None, false);
        let mailbox = Arc::new(ProjectionMailbox::default());
        mailbox
            .publish_committed(crate::acp::intent::ProjectionChangedV1 {
                project_id: SageProjectId::parse("project-foreign").unwrap(),
                chat_id: SageChatId::parse("chat-foreign").unwrap(),
                session_id: SageAgentSessionId::parse("session-foreign").unwrap(),
                projection_revision: ProjectionRevision::new(1).unwrap(),
                child_projection_epoch: 0,
            })
            .unwrap();
        let coordinator =
            SendCoordinator::new(Arc::new(Authority(snapshot(None))), handle.clone(), mailbox);
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        let runner = runner.spawn();

        let Err(mut failure) = coordinator.prepare(&tree, command()).await else {
            panic!("bootstrap failure must retain recovery")
        };
        assert!(matches!(failure.error, CoordinatorError::BootstrapRequired));
        let recovery = failure.recovery.take().expect("committed dispatch");
        let mut dispatch = match recovery.committed_dispatch() {
            Ok(dispatch) => dispatch,
            Err(_) => panic!("known Turn"),
        };
        assert_eq!(dispatch.turn_id(), &SageTurnId::parse("turn-one").unwrap());
        assert!(matches!(
            tree.reserve_turn(),
            Err(crate::acp::scheduler::SchedulerError::TurnCapacity)
        ));
        assert!(matches!(
            dispatch
                .observe(
                    SendObservation::ProcessLost,
                    None,
                    super::journal::now_millis() as i64
                )
                .await,
            Err(CoordinatorError::BootstrapRequired)
        ));
        tree.reserve_turn()
            .unwrap()
            .abort_before_durable_commit()
            .unwrap();
        handle.close_when_drained();
        runner.await.unwrap();
    });
}

#[test]
fn stale_generation_is_rejected_before_journal_or_scheduler_mutation() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (handle, _runner, order) = fake(None, false);
        let mut stale = snapshot(None);
        stale.project_generation = ProjectGeneration::new(5).unwrap();
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(stale)),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        assert!(matches!(
            coordinator.prepare(&tree, command()).await,
            Err(failure) if matches!(failure.error, CoordinatorError::Intent)
        ));
        assert!(!scheduler.active().unwrap().turn_reserved);
        assert!(order.lock().unwrap().is_empty());
    });
}

#[test]
fn exact_duplicate_bypasses_an_occupied_turn_slot() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let mut duplicate = prepared(7);
        duplicate.disposition = MutationDisposition::Duplicate;
        let (handle, runner, _) = fake(Some(duplicate.clone()), false);
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(snapshot(Some(duplicate.turn_id.clone())))),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        let _occupied = tree
            .reserve_turn()
            .unwrap()
            .bind(duplicate.turn_id.clone())
            .unwrap();
        let runner_task = runner.spawn();
        assert!(matches!(
            coordinator.prepare(&tree, command()).await.unwrap(),
            PrepareDispatch::Duplicate(_)
        ));
        handle.close_when_drained();
        runner_task.await.unwrap();
    });
}

#[test]
fn known_atomic_store_rejection_releases_only_the_turn_reservation() {
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let (handle, runner, _) = fake(None, true);
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(snapshot(None))),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        let runner_task = runner.spawn();
        let result = coordinator.prepare(&tree, command()).await;
        assert!(matches!(
            result,
            Err(failure) if matches!(
                failure.error,
                CoordinatorError::Journal(JournalError::Store(StoreError::Capacity { .. }))
            )
        ));
        assert!(tree.reserve_turn().is_ok());
        handle.close_when_drained();
        runner_task.await.unwrap();
    });
}

#[test]
fn mailbox_contention_requests_bootstrap_without_undoing_commits() {
    let mailbox = ProjectionMailbox::default();
    let first = crate::acp::intent::ProjectionChangedV1 {
        project_id: SageProjectId::parse("one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        projection_revision: ProjectionRevision::new(1).unwrap(),
        child_projection_epoch: 0,
    };
    let other = crate::acp::intent::ProjectionChangedV1 {
        project_id: SageProjectId::parse("two").unwrap(),
        chat_id: SageChatId::parse("chat-two").unwrap(),
        session_id: SageAgentSessionId::parse("session-two").unwrap(),
        projection_revision: ProjectionRevision::new(1).unwrap(),
        child_projection_epoch: 0,
    };
    assert_eq!(
        mailbox.publish_committed(first).unwrap(),
        PublishDisposition::Published
    );
    assert_eq!(
        mailbox.publish_committed(other).unwrap(),
        PublishDisposition::BootstrapRequired
    );
}

#[test]
fn adapter_cursor_progression_releases_capacity_only_after_exact_durable_terminal() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (handle, runner, _) = fake(None, false);
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(snapshot(None))),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        let runner_task = runner.spawn();
        let PrepareDispatch::Applied(mut active) =
            coordinator.prepare(&tree, command()).await.unwrap()
        else {
            panic!("new Turn must apply")
        };
        let now = super::journal::now_millis() as i64;
        active.begin_send(now).await.unwrap();
        active
            .observe(SendObservation::Correlated, None, now.saturating_add(1))
            .await
            .unwrap();

        let waiting = cursor(
            "turn-one",
            crate::acp::lifecycle::TurnState::WaitingModel,
            3,
            4,
        );
        assert_eq!(
            active.accept_adapter_commit(&waiting).unwrap(),
            MutationDisposition::Applied
        );
        assert!(matches!(
            tree.reserve_turn(),
            Err(crate::acp::scheduler::SchedulerError::TurnCapacity)
        ));

        let foreign = cursor(
            "turn-foreign",
            crate::acp::lifecycle::TurnState::Completed,
            4,
            5,
        );
        assert!(matches!(
            active.accept_adapter_commit(&foreign),
            Err(CoordinatorError::AdapterCursorForeign)
        ));
        assert!(matches!(
            tree.reserve_turn(),
            Err(crate::acp::scheduler::SchedulerError::TurnCapacity)
        ));

        let completed = cursor(
            "turn-one",
            crate::acp::lifecycle::TurnState::Completed,
            4,
            5,
        );
        assert_eq!(
            active.accept_adapter_commit(&completed).unwrap(),
            MutationDisposition::Applied
        );
        assert_eq!(
            active.accept_adapter_commit(&completed).unwrap(),
            MutationDisposition::Duplicate
        );
        tree.reserve_turn()
            .unwrap()
            .abort_before_durable_commit()
            .unwrap();
        assert!(matches!(
            active.accept_adapter_commit(&waiting),
            Err(CoordinatorError::AdapterCursorStale)
        ));
        handle.close_when_drained();
        runner_task.await.unwrap();
    });
}

#[test]
fn cancel_intent_commits_on_control_lane_before_adapter_queue_and_replays_once() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (handle, runner, order) = fake(None, false);
        let coordinator = SendCoordinator::new(
            Arc::new(Authority(snapshot(None))),
            handle.clone(),
            Arc::new(ProjectionMailbox::default()),
        );
        let scheduler = Scheduler::new();
        let tree = scheduler
            .reserve_tree(RuntimeOwner {
                project_id: command().project_id.clone(),
                chat_id: command().chat_id.clone(),
                session_id: command().session_id.clone(),
                runtime_epoch: command().runtime_epoch,
            })
            .unwrap();
        let runner_task = runner.spawn();
        let PrepareDispatch::Applied(mut active) =
            coordinator.prepare(&tree, command()).await.unwrap()
        else {
            panic!("new Turn must apply")
        };
        let now = super::journal::now_millis() as i64;
        active.begin_send(now).await.unwrap();
        active
            .observe(SendObservation::Correlated, None, now.saturating_add(1))
            .await
            .unwrap();
        let intent = ClientIntentId::parse("cancel-one").unwrap();
        let first = active
            .begin_cancel(intent.clone(), now.saturating_add(2))
            .await
            .unwrap();
        order.lock().unwrap().push("adapterCancelQueued".into());
        let duplicate = active
            .begin_cancel(intent, now.saturating_add(3))
            .await
            .unwrap();
        assert_eq!(first.disposition, MutationDisposition::Applied);
        assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
        assert_eq!(first.revision, duplicate.revision);
        let recorded = order.lock().unwrap().clone();
        let commit = recorded
            .iter()
            .position(|value| value == "cancel:cancel-one")
            .unwrap();
        let queued = recorded
            .iter()
            .position(|value| value == "adapterCancelQueued")
            .unwrap();
        assert!(commit < queued);
        assert!(matches!(
            active
                .begin_cancel(
                    ClientIntentId::parse("cancel-other").unwrap(),
                    now.saturating_add(4),
                )
                .await,
            Err(CoordinatorError::Journal(JournalError::Store(
                StoreError::Conflict {
                    resource: "cancelIntent"
                }
            )))
        ));
        assert!(matches!(
            tree.reserve_turn(),
            Err(crate::acp::scheduler::SchedulerError::TurnCapacity)
        ));
        handle.close_when_drained();
        runner_task.await.unwrap();
    });
}

fn cursor(
    turn_id: &str,
    state: crate::acp::lifecycle::TurnState,
    revision: u64,
    projection: u64,
) -> AdapterTurnCursor {
    AdapterTurnCursor::test_only(
        SageTurnId::parse(turn_id).unwrap(),
        state,
        crate::acp::lifecycle::DeliveryState::Correlated,
        EntityRevision::new(revision).unwrap(),
        ProjectionRevision::new(projection).unwrap(),
    )
}
