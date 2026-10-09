use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId, SageTurnId},
    intent::ProjectionChangedV1,
    runtime::{
        ActivatedSession, ActiveTurnIntent, AdapterPump, CompletedActivationDriverParts,
        EgressTransport, IdleActivationParts, JournalHandle, ProjectionPublishError,
        ProjectionPublishStatus, ProjectionPublisher, PumpIntent, SessionPumpIntent,
        SqliteAdapterBatchJournal, StdinTransport, SystemDriverClock,
    },
    scheduler::RuntimeOwner,
    store::DatabaseAcpStore,
    transport::{FrameWriteOutcome, OutboundFrame},
};
use std::sync::{Arc, Mutex};

pub(super) type Pump = AdapterPump<ObservedPi, SqliteAdapterBatchJournal, Mapper>;
pub(super) type ActivationIdle =
    CompletedActivationDriverParts<ObservedPi, (), Mapper, Capture, ()>;

pub(super) struct ObservedPi {
    pub(super) inner: PiRuntime,
    observed: Arc<Mutex<Vec<AdapterEvent>>>,
}

impl HarnessAdapter for ObservedPi {
    fn capabilities(&self) -> Option<CapabilitySnapshotV1> {
        self.inner.capabilities()
    }
    fn session_config(&self) -> Option<crate::acp::session_config::SessionConfig> {
        self.inner.session_config()
    }
    fn config_pending(&self) -> bool {
        self.inner.config_pending()
    }
    fn configure(
        &mut self,
        intent: &crate::acp::session_config::ConfigIntent,
    ) -> Result<Option<AdapterEgress>, AdapterRuntimeError> {
        self.inner.configure(intent)
    }
    fn command(&mut self, command: AdapterCommand) -> Result<AdapterEgress, AdapterRuntimeError> {
        self.inner.command(command)
    }
    fn ingest(
        &mut self,
        frame: &Frame,
        resources: &dyn ResourceMapper,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        let batch = self.inner.ingest(frame, resources)?;
        let mut observed = self.observed.lock().unwrap();
        assert!(observed.len() + batch.events().len() <= 4096);
        observed.extend_from_slice(batch.events());
        Ok(batch)
    }
    fn commit_mandatory_egress(&mut self, token: &EgressToken) -> Result<(), AdapterRuntimeError> {
        self.inner.commit_mandatory_egress(token)
    }
    fn resolve_egress(
        &mut self,
        token: EgressToken,
        resolution: EgressResolution,
    ) -> Result<(), AdapterRuntimeError> {
        self.inner.resolve_egress(token, resolution)
    }
}

pub(super) fn ready_adapter() -> ObservedPi {
    let mut inner = fixture_runtime(91);
    let levels = ready_until_levels(&mut inner);
    inner
        .ingest(
            &response(&levels, json!({"levels":["off","high"]})),
            &Mapper,
        )
        .unwrap();
    let models = inner
        .command(AdapterCommand::ModelPage { cursor: None })
        .unwrap();
    let models = deliver(&mut inner, models);
    inner.ingest(&response(&models,json!({"models":[{"provider":"openai","id":"gpt","name":"GPT"},{"provider":"anthropic","id":"claude","name":"Claude"}]})),&Mapper).unwrap();
    ObservedPi {
        inner,
        observed: Arc::new(Mutex::new(Vec::new())),
    }
}

pub(super) fn activated_adapter() -> ObservedPi {
    let mut inner = fixture_runtime(91);
    initialize(&mut inner);
    let session = inner
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let session = deliver(&mut inner, session);
    let batch = inner
        .ingest(&response(&session, Value::Null), &Mapper)
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { .. }]
    ));
    assert!(batch.egress().is_empty());
    ObservedPi {
        inner,
        observed: Arc::new(Mutex::new(Vec::new())),
    }
}

pub(super) fn owner() -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-pump-pi").unwrap(),
        chat_id: SageChatId::parse("chat-pump-pi").unwrap(),
        session_id: SageAgentSessionId::parse("session-pi-91").unwrap(),
        runtime_epoch: RuntimeEpoch::new(91).unwrap(),
    }
}

pub(super) fn activation_idle(reset: bool) -> ActivationIdle {
    activation_with(ready_adapter(), reset)
}

pub(super) fn activation_with(mut adapter: ObservedPi, reset: bool) -> ActivationIdle {
    if reset {
        let egress = adapter
            .inner
            .command(AdapterCommand::NewSession {
                native_root: r"C:\repo".into(),
                model: None,
            })
            .unwrap();
        let command = deliver(&mut adapter.inner, egress);
        adapter
            .inner
            .ingest(&response(&command, Value::Null), &Mapper)
            .unwrap();
    }
    let session = ActivatedSession {
        binding_digest: [7; 32],
        project_id: owner().project_id,
        chat_id: owner().chat_id,
        session_id: owner().session_id,
        external_session_id: adapter.inner.external_session.clone().unwrap(),
        runtime_epoch: owner().runtime_epoch,
        capabilities: adapter.capabilities().unwrap(),
        models: Vec::new(),
        next_model_cursor: None,
    };
    ActivationIdle {
        parts: IdleActivationParts::new(adapter, (), Mapper, session),
        idle_decoder: IncrementalFrameDecoder::new(),
        transport: Capture::default(),
        control: (),
        owner: owner(),
        clock: Arc::new(SystemDriverClock),
    }
}

#[derive(Default)]
pub(super) struct Capture(pub(super) Vec<Value>);
impl EgressTransport for Capture {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0
            .push(serde_json::from_slice(frame.payload()).unwrap());
        assert!(self.0.len() <= 16);
        FrameWriteOutcome::Complete
    }
}

impl StdinTransport for Capture {
    fn close(&mut self) {}
}

struct Publisher;
impl ProjectionPublisher for Publisher {
    fn publish_committed(
        &self,
        _: ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        Ok(ProjectionPublishStatus::Published)
    }
}

pub(super) struct Fixture {
    pub(super) database: crate::persistence::Database,
    pub(super) observed: Arc<Mutex<Vec<AdapterEvent>>>,
    external_session_id: ExternalId,
    handle: JournalHandle,
    task: Option<tokio::task::JoinHandle<()>>,
}

impl Fixture {
    pub(super) fn new() -> Self {
        Self::with_external_session_id(ExternalId::new("session", "pi-session-91-1").unwrap())
    }

    pub(super) fn with_external_session_id(external_session_id: ExternalId) -> Self {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        {
            let connection = database.connection();
            connection
                .execute_batch(
                    "INSERT INTO projects VALUES('project-pump-pi','Pi',0,0,0);
                 INSERT INTO chats VALUES('chat-pump-pi','project-pump-pi',0,'Pi',0,0,0);",
                )
                .unwrap();
            connection.execute(
                "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,created_at,updated_at)
                 VALUES('session-pi-91','project-pump-pi','chat-pump-pi',0,'pi',?1,?2,'{}','active','hot',0,0)",
                rusqlite::params!["07".repeat(32), external_session_id.as_str()],
            ).unwrap();
            connection.execute_batch(
                "INSERT INTO chat_session_heads VALUES('project-pump-pi','chat-pump-pi','session-pi-91',0,0);
             INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at) VALUES('project-pump-pi','chat-pump-pi','session-pi-91',91,'running',0);
             INSERT INTO acp_project_usage VALUES('project-pump-pi',1,1,0,0,0,0,0);
             INSERT INTO acp_session_usage VALUES('project-pump-pi','chat-pump-pi','session-pi-91',0,0,0,0,0,0,0);
             UPDATE acp_store_usage SET chat_count=1,session_count=1;"
            ).unwrap();
        }
        let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
        let fixture = Self {
            database,
            observed: Arc::new(Mutex::new(Vec::new())),
            external_session_id,
            handle,
            task: Some(runner.spawn()),
        };
        fixture.add_turn(0);
        fixture
    }

    pub(super) fn add_turn(&self, ordinal: u64) {
        self.database.connection().execute_batch(&format!(
            "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at)
             VALUES('turn-pump-{ordinal}','project-pump-pi','chat-pump-pi','session-pi-91',91,{ordinal},'turn-pump-{ordinal}','running','correlated',0,0,0);
             UPDATE acp_session_usage SET turn_count=turn_count+1;
             UPDATE acp_project_usage SET turn_count=turn_count+1;
             UPDATE acp_store_usage SET turn_count=turn_count+1;" )).unwrap();
    }

    pub(super) fn intent(&self, ordinal: u64) -> PumpIntent {
        SessionPumpIntent {
            binding_digest: [7; 32],
            project_id: owner().project_id,
            chat_id: owner().chat_id,
            session_id: owner().session_id,
            external_session_id: self.external_session_id.clone(),
            runtime_epoch: owner().runtime_epoch,
        }
        .bind_turn(
            ActiveTurnIntent::store_minted(
                SageTurnId::parse(format!("turn-pump-{ordinal}")).unwrap(),
            )
            .unwrap(),
        )
    }

    pub(super) fn pump(&self, mut adapter: ObservedPi) -> Pump {
        adapter.observed = self.observed.clone();
        let intent = self.intent(0);
        assert_eq!(
            adapter.inner.external_session.as_ref(),
            Some(&intent.external_session_id)
        );
        self.pump_adapter(adapter)
    }

    pub(super) fn pump_adapter<A: HarnessAdapter>(
        &self,
        adapter: A,
    ) -> AdapterPump<A, SqliteAdapterBatchJournal, Mapper> {
        let intent = self.intent(0);
        AdapterPump::new(
            adapter,
            SqliteAdapterBatchJournal::new(self.handle.clone(), intent.clone()),
            Mapper,
            Arc::new(Publisher),
            intent,
        )
        .unwrap()
    }

    pub(super) fn item_count(&self) -> u64 {
        self.database
            .connection()
            .query_row(
                "SELECT count(*) FROM items WHERE origin='primary'",
                [],
                |row| row.get::<_, i64>(0),
            )
            .unwrap()
            .try_into()
            .unwrap()
    }

    pub(super) async fn finish(mut self) {
        self.handle.close_when_drained();
        self.task.take().unwrap().await.unwrap();
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        self.handle.shutdown();
    }
}

pub(super) fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}
pub(super) fn reply(command: &Value, data: Value) -> Vec<u8> {
    line(
        json!({"type":"response","id":command["id"],"command":command["type"],"success":true,"data":data}),
    )
}
pub(super) async fn start<A: HarnessAdapter>(
    pump: &mut AdapterPump<A, SqliteAdapterBatchJournal, Mapper>,
) {
    start_turn(pump, 0).await;
}
pub(super) async fn start_turn<A: HarnessAdapter>(
    pump: &mut AdapterPump<A, SqliteAdapterBatchJournal, Mapper>,
    ordinal: u64,
) {
    pump.queue_command(AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Inspect").unwrap(),
        client_message_id: RequiredText::new("client", format!("turn-pump-{ordinal}")).unwrap(),
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    })
    .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    pump.push_stdout_chunk(&reply(&transport.0[0], Value::Null), 1)
        .await
        .unwrap();
    pump.push_stdout_chunk(&line(json!({"type":"agent_start"})), 2)
        .await
        .unwrap();
}

pub(super) async fn settle<A: HarnessAdapter>(
    pump: &mut AdapterPump<A, SqliteAdapterBatchJournal, Mapper>,
) {
    pump.push_stdout_chunk(&line(json!({"type":"agent_end","messages":[]})), 20)
        .await
        .unwrap();
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 21)
        .await
        .unwrap();
    assert!(pump.completed_turn_ready());
}

pub(super) fn tool_start(id: &str) -> Value {
    json!({"type":"tool_execution_start","toolCallId":id,"toolName":"read","args":{"path":"src/lib.rs"}})
}
pub(super) fn tool_end(id: &str, content: Value) -> Value {
    json!({"type":"tool_execution_end","toolCallId":id,"toolName":"read","result":{"content":content},"isError":false})
}
