use super::*;
use crate::chat_lifecycle::store::ChatLifecycleStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_acp_protocol::contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId};

#[path = "live_lanes_test_engine.rs"]
mod engine;

impl AcpService {
    pub(in crate::acp::service) fn claim_lane_activation_for_test(
        &self,
        owner: &RuntimeOwner,
    ) -> ActivationClaim<'_> {
        let lane = self
            .lanes
            .iter()
            .find(|lane| lane.holds(owner))
            .expect("a lane serves the owner");
        assert!(
            lane.activating
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
        );
        ActivationClaim { lane }
    }

    pub(in crate::acp::service) fn lane_busy_refusals_for_test(&self) -> usize {
        self.lane_busy_refusals.load(Ordering::Acquire)
    }

    pub(in crate::acp::service) fn hold_lane_until_busy_for_test(&self, owner: &RuntimeOwner) {
        std::mem::forget(self.claim_lane_activation_for_test(owner));
        self.yield_lane_on_busy.store(true, Ordering::Release);
    }

    pub(in crate::acp::service) fn control_running_refusals_for_test(&self) -> usize {
        self.control_running_refusals.load(Ordering::Acquire)
    }

    pub(super) fn observe_control_running_for_test(&self) {
        self.control_running_refusals.fetch_add(1, Ordering::AcqRel);
    }

    pub(super) fn observe_lane_busy_for_test(&self, lane: &LiveLane) {
        self.lane_busy_refusals.fetch_add(1, Ordering::AcqRel);
        if self.yield_lane_on_busy.swap(false, Ordering::AcqRel) {
            lane.activating.store(false, Ordering::Release);
        }
    }
}

fn activation_deadline() -> Instant {
    Instant::now() + Duration::from_secs(5)
}

fn service() -> AcpService {
    let database =
        crate::database::open_database(std::path::Path::new(":memory:")).expect("database");
    AcpService::new(database).expect("service")
}

fn owner(session: &str, epoch: u64) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-lanes").unwrap(),
        chat_id: SageChatId::parse(format!("chat-{session}")).unwrap(),
        session_id: SageAgentSessionId::parse(format!("session-{session}")).unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}

#[test]
fn the_lane_count_is_the_frozen_live_tree_limit_and_above_one() {
    let service = service();
    assert_eq!(service.lanes.len(), LIVE_RUNTIME_LANES);
    assert_eq!(
        LIVE_RUNTIME_LANES as u64,
        sage_core::limits::LIMITS.process.operational_live_trees
    );
    const { assert!(LIVE_RUNTIME_LANES > 1, "B2 requires concurrent live chats") };
}

#[test]
fn concurrent_activations_claim_distinct_lanes_until_capacity_is_reached() {
    let service = service();
    let mut claims = Vec::new();
    for index in 0..LIVE_RUNTIME_LANES {
        let candidate = owner(&format!("a{index}"), 1);
        let (lane, claim) = service
            .claim_activation_lane(&candidate, activation_deadline())
            .expect("a free lane");
        lane.runtime.reserve_start(candidate.clone()).unwrap();
        assert_eq!(
            service.lane_for(&candidate).unwrap().runtime.owner(),
            Some(candidate)
        );
        claims.push(claim);
    }
    assert!(matches!(
        service.claim_activation_lane(&owner("overflow", 1), activation_deadline()),
        Err(CodexSessionError::Capacity)
    ));
    drop(claims);
    assert!(matches!(
        service.claim_activation_lane(&owner("overflow", 1), activation_deadline()),
        Err(CodexSessionError::Capacity)
    ));
}

#[test]
fn a_reactivated_session_returns_to_the_lane_that_holds_its_older_epoch() {
    let service = service();
    let first = owner("same", 1);
    let (lane, claim) = service
        .claim_activation_lane(&owner("other", 1), activation_deadline())
        .unwrap();
    drop(claim);
    drop(lane);
    let (held, claim) = service
        .claim_activation_lane(&first, activation_deadline())
        .unwrap();
    held.runtime.reserve_start(first.clone()).unwrap();
    drop(claim);
    let (again, _claim) = service
        .claim_activation_lane(&owner("same", 2), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&again.runtime, &held.runtime));
}

#[test]
fn an_unknown_owner_routes_to_no_lane() {
    let service = service();
    let held = owner("held", 1);
    service
        .lane_at(1)
        .runtime
        .reserve_start(held.clone())
        .unwrap();
    assert!(service.lane_for(&owner("unknown", 1)).is_none());
    assert!(matches!(
        service.host_for(&owner("unknown", 1)),
        Err(RuntimeSlotError::Foreign)
    ));
    assert!(Arc::ptr_eq(
        &service.lane_for(&held).unwrap().runtime,
        &service.lane_at(1).runtime
    ));
}

#[test]
fn durable_activation_admits_at_most_the_live_tree_limit() {
    use crate::acp::store::{ActivateSessionCommand, StartSessionCommand, StoreError};
    use sage_acp_protocol::contract::ClientIntentId;
    tauri::async_runtime::block_on(async {
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        let root = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        let project_id = SageProjectId::parse(project.id.clone()).unwrap();
        let service = AcpService::new_with_registry_publisher_for_test(
            database.clone(),
            Arc::new(crate::acp::bindings::registry::BindingRegistry::default()),
            Arc::new(AlwaysPublished),
        )
        .unwrap();
        // Chats are created before any session binds a test-only harness.
        let chats: Vec<SageChatId> = (0..=LIVE_RUNTIME_LANES)
            .map(|_| {
                let snapshot = database.bootstrap_snapshot().unwrap();
                SageChatId::parse(
                    database
                        .create_chat(&sage_domain::chat::ProjectLayoutRequest {
                            project_id: project.id.clone(),
                            tabs: snapshot.tabs,
                            active_tab_id: snapshot.active_tab_id,
                            selected_root_id: snapshot.selected_root_id,
                            selected_resource: snapshot.selected_resource,
                        })
                        .unwrap(),
                )
                .unwrap()
            })
            .collect();
        let mut leases = Vec::new();
        for (index, chat_id) in chats.into_iter().enumerate() {
            let revision = service
                .load_chat_page(project_id.clone(), chat_id.clone(), None)
                .unwrap()
                .projection_revision;
            let started = service
                .session_starts
                .start_session(StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: sage_acp_protocol::intent::BindingId::new(
                        "bindingId",
                        "test-lanes",
                    )
                    .unwrap(),
                    binding_digest: "11".repeat(32),
                    harness_version: None,
                    model_id: None,
                    now_millis: sage_core::time::now_millis(),
                })
                .await
                .unwrap();
            let command = ActivateSessionCommand {
                project_id: project_id.clone(),
                chat_id,
                expected_projection_revision: started.projection_revision,
                client_intent_id: ClientIntentId::new(),
                session_id: started.session_id,
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                artifact_digest: "22".repeat(32),
                now_millis: sage_core::time::now_millis(),
            };
            if index < LIVE_RUNTIME_LANES {
                leases.push(
                    service
                        .lane_at(index)
                        .sessions
                        .activate_session(command)
                        .await
                        .unwrap(),
                );
            } else {
                let failure = service
                    .session_starts
                    .activate_session(command)
                    .await
                    .expect_err("the store refuses a live tree beyond the limit");
                assert!(matches!(
                    failure.error,
                    crate::acp::runtime::SessionCoordinatorError::Journal(
                        crate::acp::runtime::JournalError::Store(StoreError::Conflict {
                            resource: "runtimeEpoch"
                        })
                    )
                ));
            }
        }
        assert_eq!(leases.len(), LIVE_RUNTIME_LANES);
    });
}
struct AlwaysPublished;

fn idle_lane(
    service: &AcpService,
    index: usize,
    resumable: bool,
) -> sage_acp_protocol::scheduler::TreeLease {
    occupied_lane(service, index, resumable, true)
}

fn occupied_lane(
    service: &AcpService,
    index: usize,
    resumable: bool,
    completed: bool,
) -> sage_acp_protocol::scheduler::TreeLease {
    occupied_lane_with(
        service,
        index,
        resumable,
        completed,
        engine::IdleEngine::default(),
    )
}

fn occupied_lane_with(
    service: &AcpService,
    index: usize,
    resumable: bool,
    completed: bool,
    engine: engine::IdleEngine,
) -> sage_acp_protocol::scheduler::TreeLease {
    let owner = owner(&format!("idle-{index}"), 1);
    let capabilities = serde_json::json!({
        "schemaVersion":1,"protocolVersion":"test","newSession":true,
        "resumeSession":resumable,"loadSession":false,"closeSession":true,
        "prompt":true,"cancel":true,"nativeSessionFork":false,"activeTurnSteer":false,
        "childConversationProjection":false,"childTranscriptRead":false,
        "collaborationEventProjection":false,"multiRootExtensionSchemaId":null,
        "modelCount":0,"configCount":0
    })
    .to_string();
    let connection = service.database.connection();
    connection.execute("INSERT OR IGNORE INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-lanes','Lanes',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES(?1,'project-lanes',?2,'Idle',0,1,1)", rusqlite::params![owner.chat_id.as_str(),index as i64]).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,created_at,updated_at) VALUES(?1,'project-lanes',?2,0,'fixture',?3,'external',?4,'ready','hot',1,1)", rusqlite::params![owner.session_id.as_str(),owner.chat_id.as_str(),"a".repeat(64),capabilities]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-lanes',?1,?2,1,'running',?3,1)",rusqlite::params![owner.chat_id.as_str(),owner.session_id.as_str(),"d".repeat(64)]).unwrap();
    if completed {
        connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES(?1,'project-lanes',?2,?3,1,0,'completed','completed','correlated',1,1,1)",rusqlite::params![format!("turn-{index}"),owner.chat_id.as_str(),owner.session_id.as_str()]).unwrap();
    }
    drop(connection);
    let host = Arc::new(
        crate::acp::runtime::RuntimeHost::start_with_shutdown_recovery(
            engine,
            Arc::new(engine::Shutdown),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.lanes[index]
        .runtime
        .claim(owner.clone(), host)
        .unwrap();
    service.lanes[index]
        .last_activation
        .store(index as u64, Ordering::Release);
    service.lanes[index].scheduler.reserve_tree(owner).unwrap()
}

#[test]
fn lru_eviction_skips_non_resumable_conversations_and_uses_the_oldest_resumable_idle() {
    let service = service();
    let _leases: Vec<_> = (0..LIVE_RUNTIME_LANES)
        .map(|index| idle_lane(&service, index, index != 0))
        .collect();
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[1].runtime));
}

#[test]
fn non_resumable_idle_conversations_report_capacity_instead_of_losing_context() {
    let service = service();
    let _leases: Vec<_> = (0..LIVE_RUNTIME_LANES)
        .map(|index| idle_lane(&service, index, false))
        .collect();
    assert!(matches!(
        service.claim_activation_lane(&owner("new", 1), activation_deadline()),
        Err(CodexSessionError::Capacity)
    ));
    let (same, _claim) = service
        .claim_activation_lane(&owner("idle-0", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&same.runtime, &service.lanes[0].runtime));
}

#[test]
fn retained_recovery_cannot_starve_resumable_idle_capacity() {
    let service = service();
    *service.lanes[0].codex_recovery.lock().unwrap() =
        Some(PendingCodexRecovery::ActivationCommitted {
            owner: owner("broken", 1),
        });
    let _leases: Vec<_> = (1..LIVE_RUNTIME_LANES)
        .map(|index| idle_lane(&service, index, true))
        .collect();
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[1].runtime));
}

#[test]
fn a_reserved_turn_is_never_an_idle_eviction_candidate() {
    let service = service();
    let leases: Vec<_> = (0..LIVE_RUNTIME_LANES)
        .map(|index| idle_lane(&service, index, true))
        .collect();
    let _turn = leases[0].reserve_turn().unwrap();
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[1].runtime));
}

#[test]
fn releasing_a_chat_with_a_reserved_turn_is_refused_and_keeps_its_runtime() {
    let service = service();
    let lease = idle_lane(&service, 0, true);
    let _turn = lease.reserve_turn().unwrap();
    let owner = owner("idle-0", 1);
    assert_eq!(
        service.release_chat_runtime(
            &owner.project_id,
            &owner.chat_id,
            explicit_close_deadline(),
            || true
        ),
        Err(ChatRuntimeRelease::TurnRunning)
    );
    assert!(service.lanes[0].scheduler.active().is_some());
    assert!(!service.lanes[0].activating.load(Ordering::Acquire));
}

fn answering_control(
    service: &AcpService,
    index: usize,
    pending: &Arc<Mutex<Option<crate::acp::runtime::NativeRpcCompletionSink>>>,
) -> std::thread::JoinHandle<Result<crate::acp::goal::NativeRpcReply, crate::acp::runtime::HostError>>
{
    let owner = owner(&format!("idle-{index}"), 1);
    let host = service.lanes[index].runtime.for_owner(&owner).unwrap();
    let control = std::thread::spawn(move || {
        host.native_rpc(crate::acp::goal::NativeRpcIntent {
            owner,
            request: crate::acp::goal::NativeRpcRequest::UsageRead,
        })
    });
    let started = Instant::now();
    while pending.lock().unwrap().is_none() {
        assert!(started.elapsed() < Duration::from_secs(1));
        std::thread::yield_now();
    }
    control
}

#[test]
fn an_idle_runtime_answering_a_control_is_passed_over_for_eviction() {
    let service = service();
    let pending = Arc::new(Mutex::new(None));
    let answering = engine::IdleEngine {
        answering: Some(Arc::clone(&pending)),
    };
    let mut leases = vec![occupied_lane_with(&service, 0, true, true, answering)];
    leases.extend((1..LIVE_RUNTIME_LANES).map(|index| idle_lane(&service, index, true)));
    let control = answering_control(&service, 0, &pending);
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[1].runtime));
    let evicted = service.lanes[1]
        .runtime
        .for_owner(&owner("idle-1", 1))
        .unwrap();
    assert!(evicted.claim_idle_park().is_none());
    pending
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Ok(crate::acp::goal::NativeRpcReply::UsageRead));
    assert_eq!(
        control.join().unwrap(),
        Ok(crate::acp::goal::NativeRpcReply::UsageRead),
        "the passed-over runtime answered its control"
    );
    drop(lane);
    assert!(
        evicted.claim_idle_park().is_some(),
        "an unused eviction lets go"
    );
}

#[test]
fn a_full_house_of_runtimes_answering_controls_says_so() {
    let service = service();
    let mut leases = Vec::new();
    let mut answers = Vec::new();
    for index in 0..LIVE_RUNTIME_LANES {
        let pending = Arc::new(Mutex::new(None));
        let answering = engine::IdleEngine {
            answering: Some(Arc::clone(&pending)),
        };
        leases.push(occupied_lane_with(&service, index, true, true, answering));
        answers.push((answering_control(&service, index, &pending), pending));
    }
    assert!(matches!(
        service.claim_activation_lane(&owner("new", 1), activation_deadline()),
        Err(CodexSessionError::CapacityAnswering)
    ));
    for (control, pending) in answers {
        let answer = pending.lock().unwrap().take().unwrap();
        answer.complete(Ok(crate::acp::goal::NativeRpcReply::UsageRead));
        assert!(control.join().unwrap().is_ok());
    }
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[0].runtime));
}

#[test]
fn archiving_a_chat_whose_harness_is_answering_a_control_says_so() {
    let service = service();
    let pending = Arc::new(Mutex::new(None));
    let answering = engine::IdleEngine {
        answering: Some(Arc::clone(&pending)),
    };
    let _lease = occupied_lane_with(&service, 0, true, true, answering);
    let control = answering_control(&service, 0, &pending);
    let owner = owner("idle-0", 1);
    let refusal = service.release_chat_runtime(
        &owner.project_id,
        &owner.chat_id,
        explicit_close_deadline(),
        || true,
    );
    assert_eq!(refusal, Err(ChatRuntimeRelease::ControlRunning));
    assert_eq!(
        ChatRuntimeRelease::ControlRunning.message("archiving"),
        "This Chat's agent is still answering a request; try archiving it again"
    );
    assert!(service.lanes[0].scheduler.active().is_some());
    pending
        .lock()
        .unwrap()
        .take()
        .unwrap()
        .complete(Ok(crate::acp::goal::NativeRpcReply::UsageRead));
    assert!(control.join().unwrap().is_ok());
}

#[test]
fn never_used_or_unavailable_idle_hosts_do_not_block_parkable_capacity() {
    let service = service();
    let _leases: Vec<_> = (0..LIVE_RUNTIME_LANES)
        .map(|index| occupied_lane(&service, index, true, index != 0))
        .collect();
    let first = owner("idle-1", 1);
    service.lanes[1].runtime.mark_recovery(&first).unwrap();
    let (lane, _claim) = service
        .claim_activation_lane(&owner("new", 1), activation_deadline())
        .unwrap();
    assert!(Arc::ptr_eq(&lane.runtime, &service.lanes[2].runtime));
}

impl crate::acp::runtime::ProjectionPublisher for AlwaysPublished {
    fn publish_committed(
        &self,
        _: crate::acp::ipc::ProjectionChangedV1,
    ) -> Result<
        crate::acp::runtime::ProjectionPublishStatus,
        crate::acp::runtime::ProjectionPublishError,
    > {
        Ok(crate::acp::runtime::ProjectionPublishStatus::Published)
    }
}
