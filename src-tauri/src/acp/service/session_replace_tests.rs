use super::*;
use crate::acp::{
    contract::{
        ClientIntentId, ProjectionRevision, RequiredText, RuntimeEpoch, SafeText, SageChatId,
        SageProjectId, SageTurnId,
    },
    runtime::{
        HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, PromptWrite, RuntimeEngine,
        RuntimeHost, RuntimeShutdownRecovery,
    },
    scheduler::TreeLease,
    store::{
        AdapterTurnCursor, ParkSessionCommand, StartSessionCommand, park_session_after_zero_process,
    },
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

#[test]
fn transition_quiesces_only_its_service_lane_and_preserves_the_other_head() {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    database.connection().execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','P',1,1,1);
        INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root','project','C:/fixture','c:/fixture',0);
        INSERT INTO chats(id,project_id,title,ordinal,created_at,updated_at) VALUES('chat','project','First',0,1,1),('other','project','Other',1,1,1);
        INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project',2,0,0,0,0,0,0);
        UPDATE acp_store_usage SET chat_count=2 WHERE singleton=1;").unwrap();
    let published = Arc::new(AtomicBool::new(false));
    let service = AcpService::new_with_registry_publisher_for_test(
        database.clone(),
        Arc::new(crate::acp::bindings::registry::BindingRegistry::default()),
        Arc::new(CommitCheckingPublisher {
            database,
            published: published.clone(),
        }),
    )
    .unwrap();
    let (first, first_parked, _first_lease) = install(&service, "chat", 0);
    let (other, other_parked, other_lease) = install(&service, "other", 1);
    let page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    let mut command = transition("chat", "switch", "binding-b", page.projection_revision);
    command.expected_head = Some(first.session_id.clone());
    service.transition_admitted_session(command).unwrap();
    let page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    assert_ne!(page.current_session_id, Some(first.session_id));
    assert!(first_parked.load(Ordering::Acquire));
    assert!(published.load(Ordering::Acquire));
    assert!(!other_parked.load(Ordering::Acquire));
    let other_page = service
        .load_chat_page(other.project_id.clone(), other.chat_id.clone(), None)
        .unwrap();
    assert_eq!(
        other_page.current_session_id,
        Some(other.session_id.clone())
    );
    let lane = service.lane_for(&other).unwrap();
    assert_eq!(lane.scheduler.active().unwrap().tree, other);
    let turn = other_lease
        .lock()
        .unwrap()
        .as_ref()
        .unwrap()
        .reserve_turn()
        .unwrap();
    turn.abort_before_durable_commit().unwrap();
    assert!(lane.runtime.for_owner(&other).unwrap().state().is_ok());
    service.quiesce_transition_owner(&other).unwrap();
    service.shutdown_blocking().unwrap();
}

fn transition(
    chat: &str,
    intent: &str,
    binding: &str,
    revision: ProjectionRevision,
) -> ReplaceSessionCommand {
    ReplaceSessionCommand {
        expected_head: None,
        confirm_unknown_delivery: false,
        start: StartSessionCommand {
            project_id: SageProjectId::parse("project").unwrap(),
            chat_id: SageChatId::parse(chat).unwrap(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::parse(intent).unwrap(),
            binding_id: RequiredText::new("bindingId", binding).unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: Some(SafeText::new("harnessVersion", "1").unwrap()),
            model_id: None,
            now_millis: 1,
        },
    }
}

type TestLease = Arc<Mutex<Option<TreeLease>>>;

fn install(
    service: &AcpService,
    chat: &str,
    index: usize,
) -> (RuntimeOwner, Arc<AtomicBool>, TestLease) {
    let command = transition(chat, "start", "binding-a", ProjectionRevision::ZERO);
    let started =
        session_replace::replace_session(&mut service.database.connection(), command).unwrap();
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project").unwrap(),
        chat_id: SageChatId::parse(chat).unwrap(),
        session_id: started.session_id,
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let connection = service.database.connection();
    connection
        .execute(
            "UPDATE agent_sessions SET state='starting' WHERE id=?1",
            [owner.session_id.as_str()],
        )
        .unwrap();
    let capabilities = serde_json::json!({"schemaVersion":1,"protocolVersion":"fixture","newSession":true,"resumeSession":false,"loadSession":false,"closeSession":true,"prompt":true,"cancel":true,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}).to_string();
    connection.execute("UPDATE agent_sessions SET state='ready',residency='hot',external_session_id='native',capabilities_json=?2 WHERE id=?1", params![owner.session_id.as_str(),capabilities]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at) VALUES('project',?1,?2,1,'running',1)", params![chat,owner.session_id.as_str()]).unwrap();
    drop(connection);
    let lane = service.lane_at(index);
    let lease = Arc::new(Mutex::new(Some(
        lane.scheduler.reserve_tree(owner.clone()).unwrap(),
    )));
    let parked = Arc::new(AtomicBool::new(false));
    let host = Arc::new(
        RuntimeHost::start_with_shutdown_recovery(
            TestEngine {
                database: service.database.clone(),
                owner: owner.clone(),
                lease: lease.clone(),
                parked: parked.clone(),
            },
            Arc::new(Shutdown),
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    lane.runtime.claim(owner.clone(), host).unwrap();
    (owner, parked, lease)
}

struct TestEngine {
    database: crate::persistence::Database,
    owner: RuntimeOwner,
    lease: TestLease,
    parked: Arc<AtomicBool>,
}

impl RuntimeEngine for TestEngine {
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
        Err(HostError::State)
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn park_zero(&mut self, _: Instant) -> Result<(), HostError> {
        let head: String = self
            .database
            .connection()
            .query_row(
                "SELECT session_id FROM chat_session_heads WHERE chat_id=?1",
                [self.owner.chat_id.as_str()],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(head, self.owner.session_id.as_str());
        park_session_after_zero_process(
            &mut self.database.connection(),
            ParkSessionCommand {
                project_id: self.owner.project_id.clone(),
                chat_id: self.owner.chat_id.clone(),
                session_id: self.owner.session_id.clone(),
                runtime_epoch: self.owner.runtime_epoch,
                exit_kind: RequiredText::new("exitKind", "testPark").unwrap(),
                now_millis: 2,
            },
        )
        .map_err(|_| HostError::State)?;
        self.lease
            .lock()
            .unwrap()
            .take()
            .unwrap()
            .zero_process_converged()
            .map_err(|_| HostError::State)?;
        self.parked.store(true, Ordering::Release);
        Ok(())
    }
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

struct Shutdown;
impl RuntimeShutdownRecovery for Shutdown {
    fn shutdown_recovery_before(
        &self,
        _: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, HostError> {
        Ok(crate::acp::process::ZeroProcessToken::for_test(1))
    }
}

struct CommitCheckingPublisher {
    database: crate::persistence::Database,
    published: Arc<AtomicBool>,
}
impl crate::acp::runtime::ProjectionPublisher for CommitCheckingPublisher {
    fn publish_committed(
        &self,
        event: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<
        crate::acp::runtime::ProjectionPublishStatus,
        crate::acp::runtime::ProjectionPublishError,
    > {
        let (head, revision): (String,i64) = self.database.connection().query_row("SELECT h.session_id,c.projection_revision FROM chat_session_heads h JOIN chats c ON c.id=h.chat_id WHERE h.project_id=?1 AND h.chat_id=?2",
            params![event.project_id.as_str(),event.chat_id.as_str()], |row|Ok((row.get(0)?,row.get(1)?))).unwrap();
        assert_eq!(head, event.session_id.as_str());
        assert_eq!(revision as u64, event.projection_revision.get());
        self.published.store(true, Ordering::Release);
        Ok(crate::acp::runtime::ProjectionPublishStatus::Published)
    }
}
