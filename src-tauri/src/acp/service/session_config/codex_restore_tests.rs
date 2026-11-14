//! A restarted Codex chat's first prompt, through the real Codex binding: its
//! model and effort choices are thread settings that only a `turn/start`
//! carries, and the options that offer them come from `model/list`, which the
//! runtime asks for as activation hands off. The prompt must not go out ahead
//! of them.
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};

use super::restore_tests::{option, owner, seed};
use super::*;
use crate::acp::{
    bindings::{
        adapter::{AdapterCommand, EgressResolution, HarnessAdapter, ResourceMapper},
        codex::{
            CodexRuntime, Platform,
            test_identity::{MANAGED_CODEX_HOME, user_agent},
        },
        runtime::NativeRuntime,
    },
    contract::{RequiredText, SageTurnId},
    intent::{ExternalId, Prompt, SafeResourceRef, UnavailableResourceReason},
    runtime::{
        ConfigCompletionSink, HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent,
        PromptWrite, RuntimeEngine, RuntimeHost,
    },
    session_config::{ConfigChannel, ConfigMutation, choices},
    store::AdapterTurnCursor,
    transport::{Frame, IncrementalFrameDecoder},
};
use crate::persistence::Database;

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Foreign)
    }
}

fn frame(value: &Value) -> Frame {
    let mut bytes = serde_json::to_vec(value).unwrap();
    bytes.push(b'\n');
    IncrementalFrameDecoder::new()
        .push_chunk(&bytes)
        .unwrap()
        .unwrap()
}

/// Writes `command` and returns the request it put on the wire.
fn send(runtime: &mut NativeRuntime, command: AdapterCommand) -> Value {
    let egress = runtime.command(command).unwrap();
    let request = serde_json::from_slice(egress.frame().payload()).unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    request
}

/// The Codex runtime of a resumed thread that runs `fixture-model` at
/// `effort`, as Codex reports it, publishing its config to `channel` as a
/// live runtime does.
fn resumed(
    owner: &RuntimeOwner,
    channel: Arc<ConfigChannel>,
    effort: Option<&str>,
) -> NativeRuntime {
    let mut runtime = NativeRuntime::new(CodexRuntime::fixture_with_identity(
        Platform::WindowsX86_64,
        [3; 32],
        owner.session_id.clone(),
    ));
    runtime.attach_config(owner.clone(), channel);
    let initialize = send(&mut runtime, AdapterCommand::Initialize);
    let batch = runtime
        .ingest(
            &frame(&json!({"id":initialize["id"],"result":{
                "userAgent":user_agent("sage", "unknown"),"codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    for egress in batch.egress() {
        runtime
            .resolve_egress(egress.token().clone(), EgressResolution::Written)
            .unwrap();
    }
    let resume = send(
        &mut runtime,
        AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", "thread-1").unwrap(),
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("modelId", "fixture-model").unwrap()),
        },
    );
    runtime
        .ingest(
            &frame(&json!({"id":resume["id"],"result":{
                "thread":{"id":"thread-1"},"model":"fixture-model","reasoningEffort":effort
            }})),
            &Mapper,
        )
        .unwrap();
    runtime
}

fn catalogue(id: &Value) -> Value {
    let efforts = json!([
        {"reasoningEffort":"low","description":"Quick"},
        {"reasoningEffort":"medium","description":"Balanced"},
        {"reasoningEffort":"high","description":"Thorough"}
    ]);
    json!({"id":id,"result":{"data":[
        {"id":"fixture-model","displayName":"Synthetic replay","isDefault":true,
         "defaultReasoningEffort":"medium","supportedReasoningEfforts":efforts},
        {"id":"fixture-model-deep","displayName":"Synthetic replay deep","isDefault":false,
         "defaultReasoningEffort":"medium","supportedReasoningEfforts":efforts}
    ],"nextCursor":null}})
}

/// A live runtime whose binding is the real Codex runtime; a config change
/// is local to it until the next `turn/start`.
struct CodexEngine(Arc<Mutex<NativeRuntime>>);

impl RuntimeEngine for CodexEngine {
    fn begin_configure_session(
        &mut self,
        intent: &ConfigIntent,
        _: std::time::Instant,
        completion: ConfigCompletionSink,
    ) -> Result<(), HostError> {
        let result = self.0.lock().unwrap().configure(intent);
        completion.complete(match result {
            Ok(None) => Ok(()),
            _ => Err(HostError::ConfigMutationRejected),
        });
        Ok(())
    }
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
        Ok(())
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
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

/// What sending a prompt does: restore, then write the `turn/start`.
fn prompt(
    service: &AcpService,
    owner: &RuntimeOwner,
    runtime: &Mutex<NativeRuntime>,
    client: &str,
) -> Value {
    service.restore_session_config(owner).unwrap();
    send(
        &mut runtime.lock().unwrap(),
        AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "continue").unwrap(),
            client_message_id: RequiredText::new("client", client).unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        },
    )
}

/// Codex runs the Turn `turn_start` asked for, `turn`, to completion.
fn complete(runtime: &Mutex<NativeRuntime>, turn_start: &Value, turn: &str) {
    let mut runtime = runtime.lock().unwrap();
    for frame_value in [
        json!({"id":turn_start["id"],"result":{"turn":{"id":turn}}}),
        json!({"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":turn}}}),
        json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":turn,"status":"completed"}
        }}),
    ] {
        runtime.ingest(&frame(&frame_value), &Mapper).unwrap();
    }
}

/// Stores `choices` for the session, as an earlier launch confirmed them.
fn store(service: &AcpService, owner: &RuntimeOwner, choices: &[ConfigMutation]) {
    let connection = service.database.connection();
    for (at, choice) in choices.iter().enumerate() {
        assert!(choices::record(&connection, &owner.session_id, choice, at as i64).unwrap());
    }
}

/// Starts `runtime` as the session's live runtime and asks for the
/// catalogue as activation hands off; returns the `model/list` request id.
fn go_live(
    service: &AcpService,
    owner: &RuntimeOwner,
    runtime: &Arc<Mutex<NativeRuntime>>,
) -> Value {
    go_live_writing(service, owner, runtime, EgressResolution::Written)
}

/// `go_live`, with the `model/list` write ending as `write`.
fn go_live_writing(
    service: &AcpService,
    owner: &RuntimeOwner,
    runtime: &Arc<Mutex<NativeRuntime>>,
    write: EgressResolution,
) -> Value {
    let slot = &service.lane_at(0).runtime;
    let list = {
        let mut runtime = runtime.lock().unwrap();
        let egress = runtime.model_config_hydration().unwrap().unwrap();
        let request: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        runtime.resolve_egress(egress.into_token(), write).unwrap();
        request["id"].clone()
    };
    let host = Arc::new(RuntimeHost::start(CodexEngine(Arc::clone(runtime))).unwrap());
    host.activate(owner.clone()).unwrap();
    slot.finish_start(owner, host).unwrap();
    slot.publish_start(owner).unwrap();
    list
}

#[test]
fn the_first_prompt_after_a_restart_carries_the_restored_model_and_effort() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    let owner = owner(1);
    store(
        &service,
        &owner,
        &[
            option("model", "fixture-model-deep"),
            option("effort", "high"),
        ],
    );
    let slot = &service.lane_at(0).runtime;
    slot.reserve_start(owner.clone()).unwrap();
    let runtime = Arc::new(Mutex::new(resumed(
        &owner,
        Arc::clone(&slot.config),
        Some("medium"),
    )));
    let list = go_live(&service, &owner, &runtime);
    // Codex answers `model/list` while the prompt is already on its way; the
    // test sends at once rather than waiting for the options to show.
    let answer = std::thread::spawn({
        let runtime = Arc::clone(&runtime);
        move || {
            std::thread::sleep(std::time::Duration::from_millis(100));
            runtime
                .lock()
                .unwrap()
                .ingest(&frame(&catalogue(&list)), &Mapper)
                .unwrap();
        }
    });
    let turn_start = prompt(&service, &owner, &runtime, "after-restart");
    answer.join().unwrap();
    assert_eq!(turn_start["method"], "turn/start");
    assert_eq!(turn_start["params"]["model"], "fixture-model-deep");
    assert_eq!(turn_start["params"]["effort"], "high");
    // Both choices were restored, so neither was replaced in the store.
    let stored = choices::load(&service.database.connection(), &owner.session_id).unwrap();
    assert!(stored.contains(&option("model", "fixture-model-deep")));
    assert!(stored.contains(&option("effort", "high")));
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_stored_effort_waits_for_codex_to_report_the_thread_effort() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    let owner = owner(1);
    store(&service, &owner, &[option("effort", "high")]);
    let slot = &service.lane_at(0).runtime;
    slot.reserve_start(owner.clone()).unwrap();
    // Codex resumes the thread without saying its effort.
    let runtime = Arc::new(Mutex::new(resumed(&owner, Arc::clone(&slot.config), None)));
    let list = go_live(&service, &owner, &runtime);
    runtime
        .lock()
        .unwrap()
        .ingest(&frame(&catalogue(&list)), &Mapper)
        .unwrap();
    // The thread shows no thinking level, and the stored one is neither sent
    // nor forgotten.
    let first = prompt(&service, &owner, &runtime, "unreported");
    assert!(first["params"].get("effort").is_none());
    let stored = |service: &AcpService| {
        choices::load(&service.database.connection(), &owner.session_id).unwrap()
    };
    assert_eq!(stored(&service), [option("effort", "high")]);
    complete(&runtime, &first, "turn-1");
    // Codex then reports the thread's effort; the next prompt restores the
    // stored one over it.
    runtime
        .lock()
        .unwrap()
        .ingest(
            &frame(&json!({"method":"thread/settings/updated","params":{
                "threadId":"thread-1","threadSettings":{"model":"fixture-model","effort":"low"}
            }})),
            &Mapper,
        )
        .unwrap();
    let next = prompt(&service, &owner, &runtime, "reported");
    assert_eq!(next["params"]["effort"], "high");
    assert_eq!(stored(&service), [option("effort", "high")]);
    service.shutdown_blocking().unwrap();
}

/// What one restore takes, as a share of the budget a wait for the
/// session's options may use: a restore that waited took all of it.
fn restore_share(service: &AcpService, owner: &RuntimeOwner) -> f64 {
    let started = std::time::Instant::now();
    service.restore_session_config(owner).unwrap();
    started.elapsed().as_secs_f64()
        / std::time::Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.ordinary_rpc)
            .as_secs_f64()
}

#[test]
fn activation_with_no_stored_choices_does_not_wait_for_the_model_list() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    let owner = owner(1);
    let slot = &service.lane_at(0).runtime;
    slot.reserve_start(owner.clone()).unwrap();
    let runtime = Arc::new(Mutex::new(resumed(
        &owner,
        Arc::clone(&slot.config),
        Some("medium"),
    )));
    // `model/list` is written and never answered.
    go_live(&service, &owner, &runtime);
    assert!(runtime.lock().unwrap().config_hydrating());
    assert!(restore_share(&service, &owner) < 0.5);
    let turn_start = prompt(&service, &owner, &runtime, "nothing-stored");
    assert_eq!(turn_start["method"], "turn/start");
    assert!(turn_start["params"].get("model").is_none());
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_rejected_model_list_write_ends_hydration() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed(&service);
    let owner = owner(1);
    store(&service, &owner, &[option("effort", "high")]);
    let slot = &service.lane_at(0).runtime;
    slot.reserve_start(owner.clone()).unwrap();
    let runtime = Arc::new(Mutex::new(resumed(
        &owner,
        Arc::clone(&slot.config),
        Some("medium"),
    )));
    go_live_writing(&service, &owner, &runtime, EgressResolution::QueueRejected);
    assert!(!runtime.lock().unwrap().config_hydrating());
    // Nothing more is coming, so neither activation nor a send waits, and
    // the thread's own model is offered.
    assert!(restore_share(&service, &owner) < 0.5);
    let config = serde_json::to_value(slot.config.live_snapshot(&owner).unwrap().config).unwrap();
    assert_eq!(config["configOptions"][0]["category"], "model");
    assert_eq!(config["configOptions"][0]["currentValue"], "fixture-model");
    let turn_start = prompt(&service, &owner, &runtime, "after-rejection");
    assert_eq!(turn_start["method"], "turn/start");
    // No page listed the model, so its effort cannot be offered; the stored
    // one is kept for when it can.
    assert_eq!(
        choices::load(&service.database.connection(), &owner.session_id).unwrap(),
        [option("effort", "high")]
    );
    service.shutdown_blocking().unwrap();
}
