use super::*;
use crate::acp::store::{StoreCommit, TurnMutationCommand};
use sage_acp_protocol::contract::*;
use sage_acp_protocol::lifecycle::{DeliveryState, TurnState};
use std::sync::mpsc::sync_channel;

struct RecordingStore(std::sync::mpsc::SyncSender<String>);
impl AcpStore for RecordingStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        let command = match &batch[0] {
            StoreMutation::LookupActiveTurn(command)
            | StoreMutation::TurnTerminal { command, .. } => command,
            _ => panic!("unexpected mutation"),
        };
        let kind = if matches!(batch[0], StoreMutation::TurnTerminal { .. }) {
            "terminal"
        } else {
            "lookup"
        };
        self.0
            .send(format!("{}:{kind}", command.chat_id.as_str()))
            .unwrap();
        Ok(StoreCommit {
            projection_revision: ProjectionRevision::ZERO,
            results: Vec::new(),
        })
    }
}

fn mutation(chat: &str) -> StoreMutation {
    StoreMutation::LookupActiveTurn(TurnMutationCommand {
        project_id: SageProjectId::parse("project").unwrap(),
        chat_id: SageChatId::parse(chat).unwrap(),
        session_id: SageAgentSessionId::parse(format!("session-{chat}")).unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse(format!("turn-{chat}")).unwrap(),
        expected_revision: EntityRevision::ZERO,
        now_millis: 1,
    })
}

fn terminal(chat: &str) -> StoreMutation {
    let StoreMutation::LookupActiveTurn(command) = mutation(chat) else {
        unreachable!()
    };
    StoreMutation::TurnTerminal {
        command,
        terminal: TurnState::Completed,
        delivery: DeliveryState::Correlated,
    }
}

fn capture_database() -> crate::persistence::Database {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    database.connection().execute_batch(
        "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('slow','project',0,'Chat',0,1,1);
         INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-slow','project','slow',0,'binding','aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa','external','{}','active','hot',0,0,0,1,1);
         INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project','slow','session-slow',0,1);
         INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','slow','session-slow',1,'running','dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd',1);
         INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-slow','project','slow','session-slow',1,0,'intent','running','correlated',0,1,1);
         INSERT INTO acp_turn_workspace_changes(project_id,session_id,client_intent_id,root_id,root_path,baseline_json,payload_bytes) VALUES('project','session-slow','intent','root','/capture','{}',131072);"
    ).unwrap();
    database
}

fn staging(database: &crate::persistence::Database) -> (bool, bool, String, i64) {
    database.connection().query_row(
        "SELECT terminal_json IS NOT NULL,result_json IS NOT NULL,baseline_json,payload_bytes FROM acp_turn_workspace_changes WHERE session_id='session-slow'",
        [], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap()
}

fn prepare_staging(
    database: &crate::persistence::Database,
    mutations: &[StoreMutation],
    budget: Duration,
) -> Result<(), StoreError> {
    crate::turn_workspace::prepare_terminal(database, mutations, budget)?;
    assert_eq!(staging(database), (true, false, "{}".to_owned(), 131072));
    Ok(())
}

fn staged_capture() -> (Arc<Prepare>, std::sync::mpsc::Receiver<()>) {
    let (prepared, staged) = sync_channel(1);
    (
        Arc::new(move |database, mutations, budget| {
            prepare_staging(database, mutations, budget)?;
            prepared.send(()).unwrap();
            Ok(())
        }),
        staged,
    )
}

#[test]
fn stalled_optional_capture_keeps_capacity_but_does_not_stall_other_chats_journal() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (committed, commits) = sync_channel(4);
    let (handle, runner) = JournalHandle::new(RecordingStore(committed));
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let handle = handle.with_workspace_tracking(database);
    let first = handle
        .enqueue_one(JournalLane::Control, terminal("slow"), 0)
        .unwrap();
    let (cleanup, started, exited) =
        CaptureActor::start(&handle, runner, Arc::new(|_, _, _| Ok(())));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let other = handle
        .enqueue_one(JournalLane::Data, mutation("other"), 0)
        .unwrap();
    assert_eq!(handle.retained_counts().1, 1);
    handle.close_when_drained();
    assert_eq!(
        commits.recv_timeout(Duration::from_secs(2)).unwrap(),
        "other:lookup"
    );
    other.blocking_committed().unwrap();
    assert_eq!(handle.retained_counts().1, 1);
    assert!(matches!(
        exited.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    cleanup.release();
    assert_eq!(
        commits.recv_timeout(Duration::from_secs(2)).unwrap(),
        "slow:terminal"
    );
    first.blocking_committed().unwrap();
    exited.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(cleanup);
    assert_eq!(handle.retained_counts(), (0, 0));
}

#[test]
fn shutdown_discards_a_capture_returning_after_the_actor_stops() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (committed, commits) = sync_channel(1);
    let (handle, runner) = JournalHandle::new(RecordingStore(committed));
    let database = capture_database();
    let handle = handle.with_workspace_tracking(database.clone());
    let ticket = handle
        .enqueue_one(JournalLane::Control, terminal("slow"), 0)
        .unwrap();
    let (prepare, staged) = staged_capture();
    let (cleanup, started, exited) = CaptureActor::start(&handle, runner, prepare);
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    handle.shutdown();
    let stopped_before_release = exited.recv_timeout(Duration::from_secs(1));
    let (result, rejected) = sync_channel(1);
    let waiter = std::thread::spawn(move || result.send(ticket.blocking_committed()).unwrap());
    let rejected_before_release = rejected.recv_timeout(Duration::from_secs(1));
    assert_eq!(handle.retained_counts(), (0, 1));
    drop(cleanup);
    staged.recv_timeout(Duration::from_secs(5)).unwrap();
    waiter.join().unwrap();
    assert!(
        stopped_before_release.is_ok(),
        "shutdown waited for optional capture worker"
    );
    assert!(
        matches!(rejected_before_release, Ok(Err(JournalError::Closed))),
        "shutdown did not close pending capture ticket before worker return: {rejected_before_release:?}"
    );
    assert!(commits.try_recv().is_err());
    assert_eq!(handle.retained_counts(), (0, 0));
    assert_eq!(staging(&database), (false, false, "{}".to_owned(), 131072));
}

#[test]
fn expired_and_abandoned_optional_captures_never_commit_or_keep_staging() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    for expired in [true, false] {
        let database = capture_database();
        let (committed, commits) = sync_channel(1);
        let (handle, runner) = JournalHandle::new(RecordingStore(committed));
        let handle = handle.with_workspace_tracking(database.clone());
        let deadline = now_millis() + 5_000;
        let ticket = handle
            .enqueue_one(JournalLane::Control, terminal("slow"), deadline)
            .unwrap();
        let (prepare, staged) = staged_capture();
        let (cleanup, started, exited) = CaptureActor::start(&handle, runner, prepare);
        started.recv_timeout(Duration::from_secs(3)).unwrap();
        if expired {
            assert!(matches!(
                ticket.blocking_committed(),
                Err(JournalError::Deadline)
            ));
        } else {
            drop(ticket);
        }
        assert_eq!(handle.retained_counts(), (0, 1));
        handle.close_when_drained();
        cleanup.release();
        exited.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(cleanup);
        staged.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(commits.try_recv().is_err());
        assert_eq!(staging(&database), (false, false, "{}".to_owned(), 131072));
    }
}

#[test]
fn saturated_optional_capture_workers_skip_fifth_capture_without_losing_capacity() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (committed, commits) = sync_channel(8);
    let (handle, runner) = JournalHandle::new(RecordingStore(committed));
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let handle = handle.with_workspace_tracking(database);
    let (cleanup, started, exited) =
        CaptureActor::start(&handle, runner, Arc::new(|_, _, _| Ok(())));
    let mut tickets = Vec::new();
    for chat in ["one", "two", "three", "four"] {
        tickets.push(
            handle
                .enqueue_one(JournalLane::Control, terminal(chat), 0)
                .unwrap(),
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
    }
    assert_eq!(handle.retained_counts(), (0, 4));
    let mut held = Vec::new();
    for _ in 4..control_count_limit() {
        let mut batch = handle.batch(JournalLane::Control).unwrap();
        batch.push(mutation("reserved")).unwrap();
        held.push(batch);
    }
    assert!(matches!(
        handle.batch(JournalLane::Control),
        Err(JournalError::Capacity { lane: "control" })
    ));
    drop(held.pop());
    let fifth = handle
        .enqueue_one(JournalLane::Control, terminal("fifth"), 0)
        .unwrap();
    assert_eq!(
        commits.recv_timeout(Duration::from_secs(2)).unwrap(),
        "fifth:terminal"
    );
    fifth.blocking_committed().unwrap();
    handle
        .enqueue_one(JournalLane::Data, mutation("probe"), 0)
        .unwrap()
        .blocking_committed()
        .unwrap();
    assert!(matches!(
        started.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    drop(held);
    assert_eq!(handle.retained_counts().1, 4);
    handle.close_when_drained();
    cleanup.release();
    for ticket in tickets {
        ticket.blocking_committed().unwrap();
    }
    exited.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(cleanup);
}

struct PausedStore {
    store: RecordingStore,
    gate: Arc<(Mutex<bool>, Condvar)>,
    entered: std::sync::mpsc::SyncSender<()>,
}

impl AcpStore for PausedStore {
    fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        self.entered.send(()).unwrap();
        let (released, _) = self
            .gate
            .1
            .wait_timeout_while(self.gate.0.lock().unwrap(), Duration::from_secs(15), |v| {
                !*v
            })
            .unwrap();
        assert!(*released, "store barrier was not released");
        self.store.commit(batch)
    }
}

struct ReleaseStore(Arc<(Mutex<bool>, Condvar)>);

impl Drop for ReleaseStore {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[test]
fn shutdown_discards_prepared_capture_queued_behind_an_executing_commit() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    for lane in [JournalLane::Data, JournalLane::Control] {
        let database = capture_database();
        let (committed, commits) = sync_channel(2);
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let (entered, store_started) = sync_channel(2);
        let (handle, runner) = JournalHandle::new(PausedStore {
            store: RecordingStore(committed),
            gate: Arc::clone(&gate),
            entered,
        });
        let handle = handle.with_workspace_tracking(database.clone());
        let terminal = handle.enqueue_one(lane, terminal("slow"), 0).unwrap();
        let (prepare, staged) = staged_capture();
        let (cleanup, started, exited) = CaptureActor::start(&handle, runner, prepare);
        let release_store = ReleaseStore(gate);
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        let other = handle
            .enqueue_one(JournalLane::Data, mutation("other"), 0)
            .unwrap();
        store_started.recv_timeout(Duration::from_secs(5)).unwrap();
        cleanup.release();
        staged.recv_timeout(Duration::from_secs(5)).unwrap();
        let state = lock(&handle.queue);
        let (state, _) = handle
            .queue
            .ready
            .wait_timeout_while(state, Duration::from_secs(5), |state| {
                !state
                    .data
                    .iter()
                    .chain(&state.control)
                    .any(|e| e.workspace_result.is_some())
            })
            .unwrap();
        assert!(
            state
                .data
                .iter()
                .chain(&state.control)
                .any(|e| e.workspace_result.is_some())
        );
        drop(state);
        handle.shutdown();
        assert!(matches!(
            terminal.blocking_committed(),
            Err(JournalError::Closed)
        ));
        assert_eq!(staging(&database), (false, false, "{}".to_owned(), 131072));
        drop(release_store);
        exited.recv_timeout(Duration::from_secs(5)).unwrap();
        other.blocking_committed().unwrap();
        assert_eq!(
            commits.recv_timeout(Duration::from_secs(2)).unwrap(),
            "other:lookup"
        );
        assert!(commits.try_recv().is_err());
        drop(cleanup);
    }
}

#[test]
fn deferred_terminal_keeps_same_owner_order_across_lanes_and_deadlines() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let (committed, commits) = sync_channel(4);
    let (handle, runner) = JournalHandle::new(RecordingStore(committed));
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let handle = handle.with_workspace_tracking(database);
    let first = handle
        .enqueue_one(JournalLane::Data, terminal("slow"), now_millis() + 30_000)
        .unwrap();
    let (cleanup, started, exited) =
        CaptureActor::start(&handle, runner, Arc::new(|_, _, _| Ok(())));
    started.recv_timeout(Duration::from_secs(5)).unwrap();
    let successor = handle
        .enqueue_one(
            JournalLane::Control,
            mutation("slow"),
            now_millis() + 20_000,
        )
        .unwrap();
    let other = handle
        .enqueue_one(JournalLane::Data, mutation("other"), 0)
        .unwrap();
    handle.close_when_drained();
    let progressed = commits.recv_timeout(Duration::from_secs(2)).unwrap();
    assert_eq!(
        progressed, "other:lookup",
        "same-owner command overtook the deferred terminal"
    );
    other.blocking_committed().unwrap();
    assert!(matches!(
        exited.try_recv(),
        Err(std::sync::mpsc::TryRecvError::Empty)
    ));
    cleanup.release();
    assert_eq!(
        commits.recv_timeout(Duration::from_secs(2)).unwrap(),
        "slow:terminal"
    );
    assert_eq!(
        commits.recv_timeout(Duration::from_secs(2)).unwrap(),
        "slow:lookup"
    );
    first.blocking_committed().unwrap();
    successor.blocking_committed().unwrap();
    exited.recv_timeout(Duration::from_secs(5)).unwrap();
    drop(cleanup);
    assert_eq!(handle.retained_counts(), (0, 0));
}

struct CaptureActor {
    handle: JournalHandle,
    gate: Arc<(Mutex<bool>, Condvar)>,
    actor: Option<std::thread::JoinHandle<()>>,
}

impl CaptureActor {
    fn start<S: AcpStore + 'static>(
        handle: &JournalHandle,
        runner: JournalRunner<S>,
        prepare: Arc<Prepare>,
    ) -> (
        Self,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Receiver<()>,
    ) {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let blocked = Arc::clone(&gate);
        let (entered, started) = sync_channel(4);
        let (exit, exited) = sync_channel(1);
        let actor = std::thread::spawn(move || {
            runner.run_with_workspace_capture(Arc::new(move |database, mutations, budget| {
                entered.send(()).unwrap();
                let (released, ready) = &*blocked;
                let (released, _) = ready
                    .wait_timeout_while(released.lock().unwrap(), Duration::from_secs(15), |v| !*v)
                    .unwrap();
                assert!(*released, "capture barrier was not released");
                drop(released);
                prepare(database, mutations, budget)
            }));
            let _ = exit.send(());
        });
        (
            Self {
                handle: handle.clone(),
                gate,
                actor: Some(actor),
            },
            started,
            exited,
        )
    }

    fn release(&self) {
        *self.gate.0.lock().unwrap() = true;
        self.gate.1.notify_all();
    }
}

impl Drop for CaptureActor {
    fn drop(&mut self) {
        self.handle.shutdown();
        self.release();
        if let Some(actor) = self.actor.take() {
            actor.join().unwrap();
        }
        let state = lock(&self.handle.queue);
        let (state, _) = self
            .handle
            .queue
            .ready
            .wait_timeout_while(state, Duration::from_secs(5), |s| {
                !s.workspace_inflight.is_empty()
            })
            .unwrap();
        assert!(state.workspace_inflight.is_empty());
        assert_eq!(
            (
                state.data_count,
                state.control_count,
                state.data_bytes,
                state.control_bytes
            ),
            (0, 0, 0, 0)
        );
    }
}

#[test]
fn optional_capture_errors_and_panics_never_prevent_terminal_commit() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    for panic in [false, true] {
        let (committed, commits) = sync_channel(1);
        let (handle, runner) = JournalHandle::new(RecordingStore(committed));
        let handle = handle.with_workspace_tracking(capture_database());
        let ticket = handle
            .enqueue_one(JournalLane::Control, terminal("slow"), 0)
            .unwrap();
        let (cleanup, started, exited) = CaptureActor::start(
            &handle,
            runner,
            Arc::new(move |_, _, _| {
                assert!(!panic, "injected capture panic");
                Err(StoreError::Corrupt("injected capture failure"))
            }),
        );
        started.recv_timeout(Duration::from_secs(5)).unwrap();
        handle.close_when_drained();
        cleanup.release();
        assert!(ticket.blocking_committed().is_ok());
        assert_eq!(
            commits.recv_timeout(Duration::from_secs(5)).unwrap(),
            "slow:terminal"
        );
        exited.recv_timeout(Duration::from_secs(5)).unwrap();
    }
}
