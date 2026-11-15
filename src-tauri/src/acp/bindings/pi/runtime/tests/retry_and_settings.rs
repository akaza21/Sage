use super::*;
use crate::acp::goal::NativeRpcReply;
use sage_acp_protocol::{
    contract::{SageChatId, SageProjectId},
    intent::session_event::{SessionActivity, SessionActivityState, SessionScopedEvent},
    scheduler::RuntimeOwner,
    session_config::{ConfigId, ConfigIntent, ConfigKind, ConfigMutation, ConfigValue},
    session_control::SessionControl,
};

fn auto_compaction(runtime: &PiRuntime) -> Option<bool> {
    let config = runtime.session_config()?;
    config
        .config_options
        .as_slice()
        .iter()
        .find(|option| option.id.as_str() == "pi.autoCompaction")
        .map(|option| match option.kind {
            ConfigKind::Boolean { current_value } => current_value,
            ref other => panic!("boolean option expected, got {other:?}"),
        })
}

fn toggle(runtime: &mut PiRuntime, enabled: bool) -> Value {
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-pi").unwrap(),
            chat_id: SageChatId::parse("chat-pi").unwrap(),
            session_id: SageAgentSessionId::parse("session-pi-1").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("configId", "pi.autoCompaction").unwrap(),
            value: ConfigValue::Boolean { value: enabled },
        },
    };
    let egress = runtime.configure(&intent).unwrap().unwrap();
    deliver(runtime, egress)
}

fn ready_with_auto_compaction(enabled: bool) -> PiRuntime {
    let mut runtime = fixture_runtime(1);
    initialize_with(
        &mut runtime,
        json!({"model":{"provider":"openai","id":"gpt","name":"GPT"},"thinkingLevel":"high",
            "autoCompactionEnabled":enabled}),
    );
    let levels = open_until_levels(&mut runtime);
    answer_levels(&mut runtime, levels);
    runtime
}

#[test]
fn auto_compact_shows_pis_setting_and_changes_it_through_pi() {
    let mut runtime = ready_with_auto_compaction(true);
    assert_eq!(auto_compaction(&runtime), Some(true));
    let request = toggle(&mut runtime, false);
    assert_eq!(
        request,
        json!({"type":"set_auto_compaction","id":request["id"],"enabled":false})
    );
    assert!(runtime.config_pending());
    assert_eq!(auto_compaction(&runtime), Some(true));
    runtime
        .ingest(&response(&request, Value::Null), &Mapper)
        .unwrap();
    assert!(!runtime.config_pending());
    assert_eq!(auto_compaction(&runtime), Some(false));
}

#[test]
fn a_refused_auto_compact_change_keeps_pis_setting() {
    let mut runtime = ready_with_auto_compaction(false);
    let request = toggle(&mut runtime, true);
    let refused = runtime.ingest(
        &frame(
            json!({"type":"response","id":request["id"],"command":"set_auto_compaction",
            "success":false,"error":"settings are read-only"}),
        ),
        &Mapper,
    );
    assert!(matches!(
        refused,
        Err(AdapterRuntimeError::ConfigMutationRejected)
    ));
    assert_eq!(auto_compaction(&runtime), Some(false));
    assert!(!runtime.config_pending());
}

#[test]
fn a_pi_that_does_not_report_auto_compaction_offers_no_toggle() {
    let mut runtime = fixture_runtime(1);
    initialize(&mut runtime);
    assert!(runtime.session_config().is_some());
    assert_eq!(auto_compaction(&runtime), None);
}

fn retry_rows(batch: &crate::acp::bindings::adapter::AdapterBatch) -> Vec<SessionActivity> {
    batch
        .events()
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Session {
                event: SessionScopedEvent::Activity(activity),
                ..
            } => Some(activity.clone()),
            _ => None,
        })
        .collect()
}

fn retry_started(runtime: &mut PiRuntime) -> SessionActivity {
    runtime
        .ingest(
            &frame(
                json!({"type":"agent_end","willRetry":true,"messages":[{"role":"assistant",
                "content":[],"stopReason":"error","errorMessage":"529 overloaded"}]}),
            ),
            &Mapper,
        )
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(
                json!({"type":"auto_retry_start","attempt":1,"maxAttempts":3,
                "delayMs":2000,"errorMessage":"529 overloaded"}),
            ),
            &Mapper,
        )
        .unwrap();
    let [activity] = retry_rows(&batch).try_into().unwrap();
    activity
}

#[test]
fn a_retry_wait_is_a_stoppable_activity_that_stops_through_abort_retry() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let waiting = retry_started(&mut runtime);
    assert_eq!(
        (
            waiting.kind.as_str(),
            waiting.state,
            waiting.stoppable,
            waiting.progress.as_ref().map(|text| text.as_str())
        ),
        (
            "retry",
            SessionActivityState::Running,
            true,
            Some("Attempt 1 of 3 failed; retrying")
        )
    );
    let stop = runtime
        .session_control(&SessionControl::StopActivity {
            activity_id: waiting.id.clone(),
        })
        .unwrap();
    let stop = deliver(&mut runtime, stop);
    assert_eq!(stop, json!({"type":"abort_retry","id":stop["id"]}));
    runtime
        .ingest(&response(&stop, Value::Null), &Mapper)
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    let ended = runtime
        .ingest(
            &frame(json!({"type":"auto_retry_end","success":false,"attempt":1,
                "finalError":"Retry cancelled"})),
            &Mapper,
        )
        .unwrap();
    let [stopped] = retry_rows(&ended).try_into().unwrap();
    assert_eq!(stopped.id, waiting.id);
    assert_eq!(
        (stopped.state, stopped.stoppable, stopped.progress),
        (SessionActivityState::Cancelled, false, None)
    );
}

#[test]
fn the_retry_activity_completes_once_pi_starts_the_retried_run() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let waiting = retry_started(&mut runtime);
    let retried = runtime
        .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
        .unwrap();
    let [done] = retry_rows(&retried).try_into().unwrap();
    assert_eq!(done.id, waiting.id);
    assert_eq!(done.state, SessionActivityState::Completed);
    assert!(!done.stoppable);
    let success = runtime
        .ingest(
            &frame(json!({"type":"auto_retry_end","success":true,"attempt":1})),
            &Mapper,
        )
        .unwrap();
    assert!(retry_rows(&success).is_empty());
    assert!(matches!(
        runtime.session_control(&SessionControl::StopActivity {
            activity_id: waiting.id
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
}

#[test]
fn only_the_running_turns_retry_can_be_stopped() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    retry_started(&mut runtime);
    for foreign in ["retry:another-turn", "task:client-pi-1", "client-pi-1"] {
        assert!(matches!(
            runtime.session_control(&SessionControl::StopActivity {
                activity_id: ExternalId::new("activityId", foreign).unwrap(),
            }),
            Err(AdapterRuntimeError::UnsupportedCommand)
        ));
    }
}

#[test]
fn a_retry_pi_reports_recovered_before_a_new_run_completes() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let waiting = retry_started(&mut runtime);
    let recovered = runtime
        .ingest(
            &frame(json!({"type":"auto_retry_end","success":true,"attempt":1})),
            &Mapper,
        )
        .unwrap();
    let [done] = retry_rows(&recovered).try_into().unwrap();
    assert_eq!(done.id, waiting.id);
    assert_eq!(done.state, SessionActivityState::Completed);
}
