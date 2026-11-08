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
struct Agent {
    options: Value,
    requests: Vec<Value>,
    refuse: bool,
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
        _: std::time::Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
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

fn owner(epoch: u64) -> RuntimeOwner {
    RuntimeOwner {
        project_id: SageProjectId::parse("project-restore").unwrap(),
        chat_id: SageChatId::parse("chat-restore").unwrap(),
        session_id: SageAgentSessionId::parse("session-restore").unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}

fn seed(service: &AcpService) {
    let connection = service.database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-restore','Restore',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-restore','project-restore',0,'Restore',0,1,1)", []).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,created_at,updated_at) VALUES('session-restore','project-restore','chat-restore',0,'claude-agent-acp',?1,'external-a','{}','ready','cold',1,1)", [&"a".repeat(64)]).unwrap();
}

fn effort(current: &str) -> Value {
    json!([{
        "id":"effort","name":"Effort","category":"thought_level","type":"select",
        "currentValue":current,
        "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]
    }])
}

/// Starts a resumed runtime for `owner` the way activation does: the session
/// reports the harness defaults before the runtime is published.
fn start(service: &AcpService, owner: &RuntimeOwner, options: Value) -> Arc<Mutex<Agent>> {
    let slot = &service.lane_at(0).runtime;
    slot.reserve_start(owner.clone()).unwrap();
    let claude = InMemoryClaude::resumed(owner, Arc::clone(&slot.config), options.clone());
    let agent = Arc::new(Mutex::new(Agent {
        options,
        requests: Vec::new(),
        refuse: false,
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

fn choose(
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

fn option(id: &str, value: &str) -> ConfigMutation {
    ConfigMutation::Option {
        config_id: ConfigId::new("configId", id).unwrap(),
        value: ConfigValue::Select {
            value: ConfigId::new("value", value).unwrap(),
        },
    }
}

fn mode(id: &str) -> ConfigMutation {
    ConfigMutation::Mode(ConfigId::new("modeId", id).unwrap())
}

fn shown_effort(service: &AcpService, owner: &RuntimeOwner) -> String {
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

fn stored(service: &AcpService) -> Vec<ConfigMutation> {
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
    service.restore_session_config(&resumed);
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
    service.restore_session_config(&resumed);
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
    service.restore_session_config(&resumed);
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
    service.restore_session_config(&resumed);
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
fn restore_waits_until_the_session_reports_options() {
    let channel = ConfigChannel::default();
    let runtime = owner(2);
    channel.reserve(runtime.clone());
    channel.update(&runtime, SessionConfig::default()).unwrap();
    channel.publish(&runtime);
    // A harness that reports its options after activation stays pending.
    assert!(channel.take_restore(&runtime).is_none());
    channel
        .update(
            &runtime,
            SessionConfig::from_wire(&json!({"configOptions":effort("low")}), true).unwrap(),
        )
        .unwrap();
    let overlay = channel.take_restore(&runtime).unwrap();
    assert_ne!(overlay.config_revision, ConfigRevision::first());
    assert!(channel.take_restore(&runtime).is_none());
}
