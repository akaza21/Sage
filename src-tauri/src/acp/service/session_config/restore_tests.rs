//! A session's confirmed config choices survive a new runtime: the resumed
//! Claude binding writes the stored value back to the harness, and values the
//! harness no longer offers or refuses fall back to the session's value.
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::*;
use crate::acp::{
    bindings::{adapter::AdapterRuntimeError, claude::InMemoryClaude},
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    runtime::{
        ConfigCompletionSink, HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
        PromptWrite, RuntimeEngine, RuntimeHost,
    },
    session_config::{
        ConfigChannel, ConfigId, ConfigKind, ConfigMutation, ConfigRevision, ConfigValue,
        SessionConfig, choices,
    },
    store::AdapterTurnCursor,
};
use crate::persistence::Database;

/// The harness side: the options it reports and every request it received.
pub(super) struct Agent {
    options: Value,
    pub(super) requests: Vec<Value>,
    pub(super) refuse: bool,
    /// The runtime fails the next change this way instead of sending it.
    pub(super) fail: Option<HostError>,
    /// When each change reached the runtime, and the deadline it was given.
    pub(super) deadlines: Vec<(std::time::Instant, std::time::Instant)>,
    /// How long the harness takes to answer each change.
    pub(super) delay: std::time::Duration,
    /// Runs once, while the next change is with the harness.
    pub(super) on_send: Option<Box<dyn FnOnce() + Send>>,
}

impl Agent {
    fn answer(&mut self, request: &Value) -> Vec<Value> {
        let id = request["id"].clone();
        if self.refuse {
            return vec![
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Invalid value"}}),
            ];
        }
        let params = &request["params"];
        match request["method"].as_str() {
            Some("session/set_config_option") => {
                for option in self.options.as_array_mut().unwrap() {
                    if option["id"] == params["configId"] {
                        option["currentValue"] = params["value"].clone();
                    } else if params["configId"] == "model" && option["id"] == "effort" {
                        // Like Claude, a model change resets the effort.
                        option["currentValue"] = json!("low");
                    }
                }
                vec![json!({"jsonrpc":"2.0","id":id,"result":{"configOptions":self.options}})]
            }
            Some("session/set_mode") => vec![
                json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":"external-a","update":{"sessionUpdate":"current_mode_update","currentModeId":params["modeId"]}}}),
                json!({"jsonrpc":"2.0","id":id,"result":{}}),
            ],
            _ => panic!("unexpected request {request}"),
        }
    }
}

/// A live runtime whose binding is the real Claude runtime, answered in
/// memory by `Agent` instead of a child process.
struct HarnessEngine {
    claude: InMemoryClaude,
    agent: Arc<Mutex<Agent>>,
}

impl HarnessEngine {
    fn configure(&mut self, intent: &ConfigIntent) -> Result<(), HostError> {
        if let Some(error) = self.agent.lock().unwrap().fail.take() {
            return Err(error);
        }
        let request = self
            .claude
            .configure(intent)
            .map_err(|_| HostError::Intent)?;
        let Some(request) = request else {
            return Ok(());
        };
        let replies = {
            let mut agent = self.agent.lock().unwrap();
            agent.requests.push(request.clone());
            agent.answer(&request)
        };
        for reply in replies {
            self.claude.receive(reply).map_err(|error| match error {
                AdapterRuntimeError::ConfigMutationRejected => HostError::ConfigMutationRejected,
                _ => HostError::RecoveryRequired,
            })?;
        }
        Ok(())
    }
}

impl RuntimeEngine for HarnessEngine {
    fn begin_configure_session(
        &mut self,
        intent: &ConfigIntent,
        deadline: std::time::Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        let (delay, on_send) = {
            let mut agent = self.agent.lock().unwrap();
            agent.deadlines.push((std::time::Instant::now(), deadline));
            (agent.delay, agent.on_send.take())
        };
        if let Some(on_send) = on_send {
            on_send();
        }
        std::thread::sleep(delay);
        completion.complete(self.configure(intent));
        Ok(())
    }
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn prepare_prompt(
        &mut self,
        _: &HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
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
    fn shutdown(&mut self, _: std::time::Instant) -> Result<(), HostError> {
        Ok(())
    }
}

pub(super) fn owner(epoch: u64) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-restore").unwrap(),
        chat_id: SageChatId::parse("chat-restore").unwrap(),
        session_id: SageAgentSessionId::parse("session-restore").unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}

pub(super) fn seed(service: &AcpService) {
    seed_binding(service, "opencode-acp");
}

fn seed_binding(service: &AcpService, binding: &str) {
    let connection = service.database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-restore','Restore',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-restore','project-restore',0,'Restore',0,1,1)", []).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,created_at,updated_at) VALUES('session-restore','project-restore','chat-restore',0,?3,?1,'external-a',?2,'ready','cold',1,1)", [&"a".repeat(64), &capabilities(), binding]).unwrap();
}

/// A capability snapshot the chat page can project.
fn capabilities() -> String {
    serde_json::to_string(&crate::acp::intent::CapabilitySnapshotV1 {
        schema_version: crate::acp::contract::SchemaVersionV1,
        protocol_version: crate::acp::contract::RequiredText::new("protocol", "1").unwrap(),
        new_session: true,
        resume_session: true,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        native_session_compact: false,
        native_session_import: false,
        native_review: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 0,
        prompt_resources: false,
        prompt_images: false,
        config_count: 0,
    })
    .unwrap()
}

pub(super) fn effort(current: &str) -> Value {
    json!([{
        "id":"effort","name":"Effort","category":"thought_level","type":"select",
        "currentValue":current,
        "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]
    }])
}

/// Starts a resumed runtime for `owner` the way activation does: the session
/// reports the harness defaults before the runtime is published.
pub(super) fn start(
    service: &AcpService,
    owner: &RuntimeOwner,
    options: Value,
) -> Arc<Mutex<Agent>> {
    open(service, owner, false, "ask", options)
}

/// Finishes starting a resumed runtime whose slot `owner` already reserved.
pub(super) fn start_reserved(
    service: &AcpService,
    owner: &RuntimeOwner,
    options: Value,
) -> Arc<Mutex<Agent>> {
    open_reserved(service, owner, false, "ask", options)
}

/// Starts a runtime that opens a new session (`new`) or resumes one, in
/// `mode`.
fn open(
    service: &AcpService,
    owner: &RuntimeOwner,
    new: bool,
    mode: &str,
    options: Value,
) -> Arc<Mutex<Agent>> {
    service
        .lane_at(0)
        .runtime
        .reserve_start(owner.clone())
        .unwrap();
    open_reserved(service, owner, new, mode, options)
}

fn open_reserved(
    service: &AcpService,
    owner: &RuntimeOwner,
    new: bool,
    mode: &str,
    options: Value,
) -> Arc<Mutex<Agent>> {
    let slot = &service.lane_at(0).runtime;
    if new {
        slot.config.mark_new_session(owner);
    }
    let claude =
        InMemoryClaude::opened(owner, Arc::clone(&slot.config), new, mode, options.clone());
    let agent = Arc::new(Mutex::new(Agent {
        options,
        requests: Vec::new(),
        refuse: false,
        fail: None,
        deadlines: Vec::new(),
        delay: std::time::Duration::ZERO,
        on_send: None,
    }));
    let host = Arc::new(
        RuntimeHost::start(HarnessEngine {
            claude,
            agent: Arc::clone(&agent),
        })
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    slot.finish_start(owner, host).unwrap();
    slot.publish_start(owner).unwrap();
    agent
}

pub(super) fn choose(
    service: &AcpService,
    owner: &RuntimeOwner,
    mutation: ConfigMutation,
) -> Result<(), HostError> {
    let revision = service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)
        .unwrap()
        .config_revision;
    service.configure_session(ConfigIntent {
        owner: owner.clone(),
        expected_config_revision: revision,
        mutation,
    })
}

pub(super) fn option(id: &str, value: &str) -> ConfigMutation {
    ConfigMutation::Option {
        config_id: ConfigId::new("configId", id).unwrap(),
        value: ConfigValue::Select {
            value: ConfigId::new("value", value).unwrap(),
        },
    }
}

pub(super) fn mode(id: &str) -> ConfigMutation {
    ConfigMutation::Mode(ConfigId::new("modeId", id).unwrap())
}

pub(super) fn shown_effort(service: &AcpService, owner: &RuntimeOwner) -> String {
    let overlay = service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(owner)
        .unwrap();
    let option = overlay
        .config
        .config_options
        .as_slice()
        .iter()
        .find(|option| option.id.as_str() == "effort")
        .unwrap()
        .clone();
    match option.kind {
        ConfigKind::Select { current_value, .. } => current_value.as_str().to_owned(),
        _ => panic!("effort is a select"),
    }
}

pub(super) fn stored(service: &AcpService) -> Vec<ConfigMutation> {
    choices::load(&service.database.connection(), &owner(1).session_id).unwrap()
}

#[test]
fn a_restarted_runtime_sends_the_stored_effort_before_the_next_prompt() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-restore-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
        seed(&service);
        let first = owner(1);
        let agent = start(&service, &first, effort("low"));
        choose(&service, &first, option("effort", "high")).unwrap();
        assert_eq!(agent.lock().unwrap().requests.len(), 1);
        assert_eq!(stored(&service), [option("effort", "high")]);
        service.shutdown_blocking().unwrap();
    }
    // Sage restarts; the session resumes with the harness default again.
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    assert_eq!(shown_effort(&service, &resumed), "low");
    service.restore_session_config(&resumed).unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "session/set_config_option");
    assert_eq!(
        requests[0]["params"],
        json!({"sessionId":"external-a","configId":"effort","value":"high"})
    );
    // The composer reads the live overlay, which now shows the choice.
    assert_eq!(shown_effort(&service, &resumed), "high");
    // Restoring happens once per runtime; a later prompt sends nothing more.
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    service.shutdown_blocking().unwrap();
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn values_no_longer_offered_or_refused_fall_back_to_the_session_without_retrying() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        let session = &owner(1).session_id;
        for (at, choice) in [
            option("effort", "max"),
            option("retired", "on"),
            mode("code"),
        ]
        .iter()
        .enumerate()
        {
            assert!(choices::record(&connection, session, choice, at as i64).unwrap());
        }
    }
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    agent.lock().unwrap().refuse = true;
    service.restore_session_config(&resumed).unwrap();
    // "max" is not offered and "retired" is gone: nothing is sent for them.
    // The offered mode is sent once, refused, and not retried.
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "session/set_mode");
    assert_eq!(requests[0]["params"]["modeId"], "code");
    assert_eq!(shown_effort(&service, &resumed), "low");
    let mut kept = stored(&service);
    kept.sort_by_key(|choice| matches!(choice, ConfigMutation::Mode(_)));
    assert_eq!(kept, [option("effort", "low"), mode("ask")]);
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    // Only a confirmed choice is stored: a refused one leaves the store alone.
    assert_eq!(
        choose(&service, &resumed, option("effort", "high")),
        Err(HostError::ConfigMutationRejected)
    );
    assert!(stored(&service).contains(&option("effort", "low")));
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_binding_whose_access_owns_its_mode_never_has_a_stored_mode_reapplied() {
    for binding in ["claude-agent-acp", "claude-code-sdk"] {
        let service =
            AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
        seed_binding(&service, binding);
        {
            let connection = service.database.connection();
            let session = &owner(1).session_id;
            // Picked before the chat's access was lowered.
            assert!(choices::record(&connection, session, &mode("code"), 1).unwrap());
            assert!(choices::record(&connection, session, &option("effort", "high"), 2).unwrap());
        }
        let resumed = owner(2);
        let agent = start(&service, &resumed, effort("low"));
        // Whenever the restore finishes, it sends no mode.
        service.restore_session_config(&resumed).unwrap();
        let requests = agent.lock().unwrap().requests.clone();
        assert_eq!(requests.len(), 1, "{binding}");
        assert_eq!(requests[0]["method"], "session/set_config_option");
        assert_eq!(requests[0]["params"]["configId"], "effort");
        service.shutdown_blocking().unwrap();
    }
}

#[test]
fn restore_waits_until_the_session_reports_options() {
    let channel = ConfigChannel::default();
    let runtime = owner(2);
    channel.reserve(runtime.clone());
    channel.update(&runtime, SessionConfig::default()).unwrap();
    channel.publish(&runtime);
    let has_options = |config: &SessionConfig| !config.config_options.as_slice().is_empty();
    // A harness that reports its options after activation stays pending.
    assert!(channel.take_restore(&runtime, &[], has_options).is_none());
    channel
        .update(
            &runtime,
            SessionConfig::from_wire(&json!({"configOptions":effort("low")}), true).unwrap(),
        )
        .unwrap();
    let (overlay, new_session) = channel.take_restore(&runtime, &[], has_options).unwrap();
    assert_ne!(overlay.config_revision, ConfigRevision::first());
    assert!(!new_session);
    assert!(channel.take_restore(&runtime, &[], |_| true).is_none());
}

#[test]
fn a_new_sessions_first_mode_survives_a_resume_that_starts_elsewhere() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-new-mode-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    seed(&service);
    // A new session starts in "default"; the user never changes the mode.
    let first = owner(1);
    let agent = open(&service, &first, true, "default", effort("low"));
    service.restore_session_config(&first).unwrap();
    assert!(agent.lock().unwrap().requests.is_empty());
    assert_eq!(stored(&service), [mode("default")]);
    service.shutdown_blocking().unwrap();
    // The harness resumes a transcript with no mode in "auto".
    drop(service);
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    let resumed = owner(2);
    let agent = open(&service, &resumed, false, "auto", effort("low"));
    service.restore_session_config(&resumed).unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["method"], "session/set_mode");
    assert_eq!(requests[0]["params"]["modeId"], "default");
    let shown = service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(&resumed)
        .unwrap();
    assert_eq!(
        shown.config.modes.unwrap().current_mode_id.as_str(),
        "default"
    );
    service.shutdown_blocking().unwrap();
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn stored_options_are_kept_while_the_session_has_not_reported_options() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        let session = &owner(1).session_id;
        assert!(choices::record(&connection, session, &mode("code"), 1).unwrap());
        assert!(choices::record(&connection, session, &option("effort", "high"), 2).unwrap());
    }
    // Modes are reported at activation; options are not yet.
    let resumed = owner(2);
    let agent = start(&service, &resumed, json!([]));
    service.restore_session_config(&resumed).unwrap();
    assert!(agent.lock().unwrap().requests.is_empty());
    assert_eq!(stored(&service), [mode("code"), option("effort", "high")]);
    service.shutdown_blocking().unwrap();
}

#[test]
fn what_the_harness_reports_itself_is_stored_once_the_runtime_is_restored() {
    let channel = ConfigChannel::default();
    let recorded = Arc::new(Mutex::new(Vec::new()));
    channel.set_recorder({
        let recorded = Arc::clone(&recorded);
        Arc::new(move |_: &RuntimeOwner, choices: &[ConfigMutation]| {
            recorded.lock().unwrap().extend_from_slice(choices)
        })
    });
    let runtime = owner(2);
    let config = |mode: &str, effort_value: &str| {
        SessionConfig::from_wire(
            &json!({
                "modes":{"currentModeId":mode,"availableModes":[
                    {"id":"plan","name":"Plan"},{"id":"acceptEdits","name":"Accept edits"}
                ]},
                "configOptions":effort(effort_value)
            }),
            true,
        )
        .unwrap()
    };
    channel.reserve(runtime.clone());
    channel.update(&runtime, config("plan", "high")).unwrap();
    channel.publish(&runtime);
    // Before restore, what the session reports is its defaults, not a choice.
    channel.update(&runtime, config("plan", "low")).unwrap();
    assert!(recorded.lock().unwrap().is_empty());
    assert!(channel.take_restore(&runtime, &[], |_| true).is_some());
    // Then the harness leaves plan mode on its own and resets the effort.
    channel
        .update(&runtime, config("acceptEdits", "high"))
        .unwrap();
    assert_eq!(
        *recorded.lock().unwrap(),
        [mode("acceptEdits"), option("effort", "high")]
    );
}

#[test]
fn an_effort_the_harness_resets_keeps_its_stored_value_until_it_is_restored() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        let session = &owner(1).session_id;
        assert!(choices::record(&connection, session, &option("effort", "high"), 1).unwrap());
        assert!(choices::record(&connection, session, &option("model", "b"), 2).unwrap());
    }
    let options = json!([{
        "id":"model","name":"Model","category":"model","type":"select","currentValue":"a",
        "options":[{"value":"a","name":"A"},{"value":"b","name":"B"}]
    }, {
        "id":"effort","name":"Effort","category":"thought_level","type":"select",
        "currentValue":"medium",
        "options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]
    }]);
    let resumed = owner(2);
    let agent = start(&service, &resumed, options);
    // The model is restored first; it resets the effort to "low", and the
    // budget runs out before the effort is reached.
    let mut sent = 0;
    service
        .restore_until(&resumed, || {
            sent += 1;
            sent > 1
        })
        .unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["params"]["configId"], "model");
    assert_eq!(shown_effort(&service, &resumed), "low");
    assert!(stored(&service).contains(&option("effort", "high")));
    assert!(stored(&service).contains(&option("model", "b")));
    // Before the next prompt the rest of the restore runs.
    service.restore_session_config(&resumed).unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(
        requests[1]["params"],
        json!({"sessionId":"external-a","configId":"effort","value":"high"})
    );
    assert_eq!(shown_effort(&service, &resumed), "high");
    // Once restored, an effort the harness resets is the session's again.
    choose(&service, &resumed, option("model", "a")).unwrap();
    assert_eq!(shown_effort(&service, &resumed), "low");
    assert!(stored(&service).contains(&option("effort", "low")));
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_choice_raced_by_another_change_is_retried_once_against_it() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        assert!(
            choices::record(
                &connection,
                &owner(1).session_id,
                &option("effort", "high"),
                1
            )
            .unwrap()
        );
    }
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    // The user switches mode just before the stored effort is sent, so it is
    // sent against a revision that is no longer current.
    let mut raced = false;
    service
        .restore_until(&resumed, || {
            if !raced {
                raced = true;
                choose(&service, &resumed, mode("code")).unwrap();
            }
            false
        })
        .unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["method"], "session/set_mode");
    assert_eq!(
        requests[1]["params"],
        json!({"sessionId":"external-a","configId":"effort","value":"high"})
    );
    assert_eq!(shown_effort(&service, &resumed), "high");
    let kept = stored(&service);
    assert!(kept.contains(&mode("code")) && kept.contains(&option("effort", "high")));
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_change_that_needs_recovery_is_reported_instead_of_the_prompt_being_sent() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        assert!(
            choices::record(
                &connection,
                &owner(1).session_id,
                &option("effort", "high"),
                1
            )
            .unwrap()
        );
    }
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    agent.lock().unwrap().fail = Some(HostError::RecoveryRequired);
    let failure = service
        .send_session_turn(HostPromptIntent {
            command: crate::acp::store::PrepareTurnCommand {
                project_id: resumed.project_id.clone(),
                project_generation: crate::acp::contract::ProjectGeneration::new(1).unwrap(),
                chat_id: resumed.chat_id.clone(),
                session_id: resumed.session_id.clone(),
                runtime_epoch: resumed.runtime_epoch,
                expected_projection_revision: crate::acp::contract::ProjectionRevision::ZERO,
                client_intent_id: crate::acp::contract::ClientIntentId::new(),
                prompt: crate::acp::intent::Prompt::new("prompt", "hello").unwrap(),
                now_millis: crate::persistence::now_millis(),
            },
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap_err();
    assert!(matches!(
        failure.error,
        crate::acp::service::CodexSessionError::Recovery
    ));
    // Nothing reached the harness, and the stored choice is kept.
    assert!(agent.lock().unwrap().requests.is_empty());
    assert_eq!(stored(&service), [option("effort", "high")]);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_choice_the_user_makes_while_the_restore_is_deferred_is_kept() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        let session = &owner(1).session_id;
        assert!(choices::record(&connection, session, &option("effort", "high"), 1).unwrap());
        assert!(choices::record(&connection, session, &mode("code"), 2).unwrap());
    }
    let resumed = owner(2);
    let agent = start(
        &service,
        &resumed,
        json!([{
            "id":"effort","name":"Effort","category":"thought_level","type":"select",
            "currentValue":"low",
            "options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]
        }]),
    );
    // The mode is restored; the budget runs out before the effort.
    let mut sends = 0;
    service
        .restore_until(&resumed, || {
            sends += 1;
            sends > 1
        })
        .unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    assert!(stored(&service).contains(&option("effort", "high")));
    // Before the next prompt the user picks an effort of their own.
    choose(&service, &resumed, option("effort", "medium")).unwrap();
    // The rest of the restore does not put the old value back.
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 2);
    assert_eq!(shown_effort(&service, &resumed), "medium");
    assert!(stored(&service).contains(&option("effort", "medium")));
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_change_whose_time_ran_out_before_it_was_sent_is_restored_later_on_a_healthy_runtime() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    {
        let connection = service.database.connection();
        assert!(
            choices::record(
                &connection,
                &owner(1).session_id,
                &option("effort", "high"),
                1
            )
            .unwrap()
        );
    }
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    // The budget is spent by the time the change reaches the runtime host.
    let spent = std::time::Instant::now();
    seam::spend_deadlines(true);
    let restored = service.restore_until(&resumed, || false);
    seam::spend_deadlines(false);
    restored.unwrap();
    assert!(agent.lock().unwrap().requests.is_empty());
    let host = service.lane_at(0).runtime.for_owner(&resumed).unwrap();
    let revision = service
        .lane_at(0)
        .runtime
        .config
        .live_snapshot(&resumed)
        .unwrap()
        .config_revision;
    let intent = ConfigIntent {
        owner: resumed.clone(),
        expected_config_revision: revision,
        mutation: option("effort", "high"),
    };
    assert_eq!(
        host.configure_session_until(intent, spent),
        Err(HostError::NotDispatched)
    );
    // The runtime is not in recovery: the rest of the restore runs before
    // the next prompt.
    assert!(!matches!(
        host.state(),
        Ok(crate::acp::runtime::HostState::Recovery) | Err(_)
    ));
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    assert_eq!(shown_effort(&service, &resumed), "high");
    assert_eq!(stored(&service), [option("effort", "high")]);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_runtime_killed_without_shutdown_leaves_its_session_unresumable_after_restart() {
    // What the laptop check hit: the app was killed while the session was
    // hot, so restart recovery fails that session (by design) and the
    // reopened chat has nothing to resume. Its stored choices are kept.
    let directory =
        std::env::temp_dir().join(format!("sage-config-killed-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    {
        let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
        seed(&service);
        let connection = service.database.connection();
        connection
            .execute_batch(
                "UPDATE agent_sessions SET residency='hot' WHERE id='session-restore';
                 INSERT INTO chat_session_heads VALUES('project-restore','chat-restore','session-restore',0,1);
                 INSERT INTO acp_session_usage VALUES('project-restore','chat-restore','session-restore',0,0,0,0,0,0,0);
                 INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project-restore',1,1,0,0,0,0,0);
                 UPDATE acp_store_usage SET chat_count=1,session_count=1 WHERE singleton=1;
                 INSERT INTO session_runtime_epochs VALUES('project-restore','chat-restore','session-restore',1,'running',NULL,1,NULL,NULL);",
            )
            .unwrap();
        assert!(
            choices::record(
                &connection,
                &owner(1).session_id,
                &option("effort", "high"),
                1
            )
            .unwrap()
        );
        // No shutdown: the process is killed here.
    }
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    let (state, residency, heads): (String, String, i64) = service
        .database
        .connection()
        .query_row(
            "SELECT state,residency,(SELECT COUNT(*) FROM chat_session_heads) FROM agent_sessions WHERE id='session-restore'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        (state.as_str(), residency.as_str(), heads),
        ("failed", "cold", 0)
    );
    assert_eq!(stored(&service), [option("effort", "high")]);
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

/// The deadline each restore change gets, observed and steered per thread:
/// the restore runs on the thread that calls it.
pub(super) mod seam {
    use std::{
        cell::{Cell, RefCell},
        time::Instant,
    };

    thread_local! {
        static SENT: RefCell<Vec<(Instant, Instant)>> = const { RefCell::new(Vec::new()) };
        static SPENT: Cell<bool> = const { Cell::new(false) };
    }

    /// Records when a change's deadline was set and what it was. While
    /// deadlines are spent, it gives one already past instead.
    pub(in crate::acp::service) fn send_deadline(set_at: Instant, deadline: Instant) -> Instant {
        SENT.with(|sent| sent.borrow_mut().push((set_at, deadline)));
        if SPENT.get() { set_at } else { deadline }
    }

    pub(in crate::acp::service) fn spend_deadlines(spent: bool) {
        SPENT.set(spent);
    }

    /// The deadlines set on this thread since the last call.
    pub(in crate::acp::service) fn take_sent() -> Vec<(Instant, Instant)> {
        SENT.with(|sent| std::mem::take(&mut *sent.borrow_mut()))
    }
}
