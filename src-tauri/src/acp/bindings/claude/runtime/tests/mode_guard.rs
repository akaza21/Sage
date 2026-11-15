use super::*;
use sage_acp_protocol::access_mode::SessionAccessMode;
use sage_acp_protocol::{
    intent::session_event::{SessionNoticeLevel, SessionScopedEvent},
    permissions::PermissionDecision,
};

const MODES: &str = r#"{"currentModeId":"plan","availableModes":[
    {"id":"default","name":"Manual"},{"id":"acceptEdits","name":"Accept edits"},
    {"id":"plan","name":"Plan"},{"id":"auto","name":"Auto"},
    {"id":"bypassPermissions","name":"Bypass permissions"}]}"#;

fn ready(runtime: &mut ClaudeRuntime) {
    initialize(runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    let modes: Value = serde_json::from_str(MODES).unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{"sessionId":"claude-session-1","modes":modes}})),
            &Mapper,
        )
        .unwrap();
}

fn prompt(
    runtime: &mut ClaudeRuntime,
    mode: SessionAccessMode,
) -> Result<AdapterEgress, AdapterRuntimeError> {
    runtime.command(AdapterCommand::Prompt {
        prompt: Prompt::new("prompt", "Plan the change").unwrap(),
        client_message_id: RequiredText::new("client", "client-message-1").unwrap(),
        access_mode: mode,
    })
}

fn planning(runtime: &mut ClaudeRuntime, mode: SessionAccessMode) {
    ready(runtime);
    let native = crate::acp::bindings::access_policy::session_mode(
        crate::acp::bindings::claude::CLAUDE_BINDING_ID,
        mode,
    )
    .unwrap();
    runtime.ingest(&mode_update(native), &Mapper).unwrap();
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Plan the change").unwrap(),
            client_message_id: RequiredText::new("client", "client-message-1").unwrap(),
            access_mode: mode,
        })
        .unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{
                "sessionId":"claude-session-1","update":{
                    "sessionUpdate":"tool_call","toolCallId":"exit-plan",
                    "kind":"execute","status":"in_progress"
                }
            }})),
            &Mapper,
        )
        .unwrap();
}

fn mode_update(mode: &str) -> Frame {
    frame(json!({"method":"session/update","params":{
        "sessionId":"claude-session-1",
        "update":{"sessionUpdate":"current_mode_update","currentModeId":mode}
    }}))
}

fn plan_exit(request: &str, options: Value) -> Frame {
    frame(json!({
        "id":request,"method":"session/request_permission","params":{
            "sessionId":"claude-session-1","toolCall":{"toolCallId":"exit-plan"},
            "options":options
        }
    }))
}

fn answer(
    runtime: &mut ClaudeRuntime,
    batch: &AdapterBatch,
    decision: PermissionDecision,
) -> Value {
    let request = batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                external_request_id,
                ..
            } => Some(external_request_id.clone()),
            _ => None,
        })
        .expect("a permission request is published");
    let reply = runtime
        .command(AdapterCommand::ResolvePermission {
            request_id: SageServerRequestId::parse(request.as_str()).unwrap(),
            decision,
        })
        .unwrap();
    serde_json::from_slice(reply.frame().payload()).unwrap()
}

fn offered(batch: &AdapterBatch) -> Vec<sage_acp_protocol::intent::PermissionOptionKind> {
    batch
        .events()
        .iter()
        .find_map(|event| match event {
            SharedEvent::PermissionRequest {
                payload:
                    NormalizedPayload::PermissionRequest {
                        offered_option_kinds,
                        ..
                    },
                ..
            } => offered_option_kinds
                .as_ref()
                .map(|kinds| kinds.as_slice().to_vec()),
            _ => None,
        })
        .unwrap_or_default()
}

#[test]
fn a_plan_exit_never_picks_an_option_that_widens_the_mode() {
    use sage_acp_protocol::intent::PermissionOptionKind::{AllowAlways, AllowOnce};
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    // With auto available the adapter offers only auto as the "always"
    // exit, first with a context clear. Neither is offered to the user.
    let batch = runtime
        .ingest(
            &plan_exit(
                "exit-auto",
                json!([
                    {"optionId":"exit-plan-clear-auto","name":"Yes, clear context and use auto mode","kind":"allow_always"},
                    {"optionId":"exit-plan-auto","name":"Yes, and use auto mode","kind":"allow_always"},
                    {"optionId":"exit-plan-default","name":"Yes, manually approve edits","kind":"allow_once"},
                    {"optionId":"reject","name":"No, keep planning","kind":"reject_once"}
                ]),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(!offered(&batch).contains(&AllowAlways));
    assert!(offered(&batch).contains(&AllowOnce));
    let reply = answer(&mut runtime, &batch, PermissionDecision::AllowOnce);
    assert_eq!(reply["result"]["outcome"]["optionId"], "exit-plan-default");

    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    let batch = runtime
        .ingest(
            &plan_exit(
                "exit-edits",
                json!([
                    {"optionId":"exit-plan-clear-accept-edits","name":"Yes, clear context and auto-accept edits","kind":"allow_always"},
                    {"optionId":"exit-plan-bypass","name":"Yes, and bypass permissions","kind":"allow_always"},
                    {"optionId":"exit-plan-accept-edits","name":"Yes, auto-accept edits","kind":"allow_always"},
                    {"optionId":"exit-plan-default","name":"Yes, manually approve edits","kind":"allow_once"}
                ]),
            ),
            &Mapper,
        )
        .unwrap();
    let reply = answer(&mut runtime, &batch, PermissionDecision::AllowAlways);
    assert_eq!(
        reply["result"]["outcome"]["optionId"],
        "exit-plan-accept-edits"
    );
    let batch = runtime
        .ingest(&mode_update("acceptEdits"), &Mapper)
        .unwrap();
    assert!(batch.egress().is_empty());
}

#[test]
fn a_mode_change_the_turn_was_not_given_cancels_it_with_a_notice() {
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    let batch = runtime
        .ingest(&mode_update("bypassPermissions"), &Mapper)
        .unwrap();
    let cancel: Value = serde_json::from_slice(batch.egress()[0].frame().payload()).unwrap();
    assert_eq!(cancel["method"], "session/cancel");
    assert_eq!(cancel["params"]["sessionId"], "claude-session-1");
    assert!(batch.events().iter().any(|event| matches!(
        event,
        SharedEvent::Session {
            event: SessionScopedEvent::Notice(notice),
            ..
        } if notice.level == SessionNoticeLevel::Error
            && notice.text.as_str().contains("bypassPermissions")
    )));
}

#[test]
fn a_narrower_mode_change_keeps_the_turn_running() {
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::AcceptEdits);
    for mode in ["acceptEdits", "plan", "default"] {
        let batch = runtime.ingest(&mode_update(mode), &Mapper).unwrap();
        assert!(batch.egress().is_empty(), "{mode}");
        assert!(matches!(batch.events(), [SharedEvent::IgnoredDuplicate]));
    }
    let batch = runtime.ingest(&mode_update("auto"), &Mapper).unwrap();
    assert_eq!(batch.egress().len(), 1);
}

#[test]
fn a_prompt_is_written_only_while_the_session_is_in_the_turns_mode() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    assert!(prompt(&mut runtime, SessionAccessMode::Ask).is_err());
    runtime.ingest(&mode_update("default"), &Mapper).unwrap();
    assert!(prompt(&mut runtime, SessionAccessMode::Ask).is_ok());
}

#[test]
fn a_running_turn_refuses_a_mode_change() {
    use sage_acp_protocol::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation},
    };
    let intent = ConfigIntent {
        owner: RuntimeOwner {
            project_id: SageProjectId::parse("project-a").unwrap(),
            chat_id: SageChatId::parse("chat-a").unwrap(),
            session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
            runtime_epoch: RuntimeEpoch::new(3).unwrap(),
        },
        expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
        mutation: ConfigMutation::Mode(ConfigId::new("modeId", "default").unwrap()),
    };
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    assert!(runtime.configure(&intent).is_ok());
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    assert!(runtime.configure(&intent).is_err());
}

#[test]
fn a_set_option_reply_that_widens_a_running_turn_cancels_it() {
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    let reply = runtime
        .config_response(
            json!({"result":{"configOptions":[{"id":"mode","name":"Mode","category":"mode",
                "type":"select","currentValue":"bypassPermissions","options":[
                {"value":"plan","name":"Plan"},{"value":"bypassPermissions","name":"Bypass"}]}]}}),
            true,
        )
        .unwrap();
    let cancel: Value = serde_json::from_slice(reply.egress()[0].frame().payload()).unwrap();
    assert_eq!(cancel["method"], "session/cancel");
}

#[test]
fn a_mode_switch_offered_under_unknown_ids_is_never_allowed() {
    use sage_acp_protocol::intent::PermissionOptionKind::{AllowAlways, AllowOnce};
    let mut runtime = ClaudeRuntime::fixture();
    planning(&mut runtime, SessionAccessMode::Plan);
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":"exit-renamed","method":"session/request_permission","params":{
                    "sessionId":"claude-session-1",
                    "toolCall":{"toolCallId":"exit-plan","kind":"switch_mode"},
                    "options":[
                        {"optionId":"plan-exit-bypass","name":"Yes, and bypass permissions","kind":"allow_always"},
                        {"optionId":"allow-once","name":"Yes","kind":"allow_once"},
                        {"optionId":"reject","name":"No, keep planning","kind":"reject_once"}
                    ]
                }
            })),
            &Mapper,
        )
        .unwrap();
    assert!(!offered(&batch).contains(&AllowAlways));
    assert!(!offered(&batch).contains(&AllowOnce));
}

/// What the pinned 0.76 adapter does on `session/set_mode`: it restates
/// the `mode` option in a `config_option_update` before its bare
/// acknowledgement, and sends `current_mode_update` only when it falls back
/// from Auto.
#[test]
fn a_set_mode_is_followed_by_the_adapters_option_report() {
    use sage_acp_protocol::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
        session_config::{ConfigId, ConfigIntent, ConfigMutation},
    };
    let option = |current: &str| {
        json!([{"id":"mode","name":"Mode","category":"mode","type":"select",
            "currentValue":current,"options":[{"value":"default","name":"Default"},
            {"value":"acceptEdits","name":"Accept edits"},{"value":"plan","name":"Plan"}]}])
    };
    let mut runtime = ClaudeRuntime::fixture();
    initialize(&mut runtime);
    let egress = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\fixture\root".into(),
            model: None,
        })
        .unwrap();
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    let modes: Value = serde_json::from_str(MODES).unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":id,"result":{
                "sessionId":"claude-session-1","modes":modes,"configOptions":option("plan")}})),
            &Mapper,
        )
        .unwrap();
    let egress = runtime
        .configure(&ConfigIntent {
            owner: RuntimeOwner {
                project_id: SageProjectId::parse("project-a").unwrap(),
                chat_id: SageChatId::parse("chat-a").unwrap(),
                session_id: SageAgentSessionId::parse("session-claude-runtime").unwrap(),
                runtime_epoch: RuntimeEpoch::new(3).unwrap(),
            },
            expected_config_revision: serde_json::from_value(json!("1")).unwrap(),
            mutation: ConfigMutation::Mode(ConfigId::new("modeId", "acceptEdits").unwrap()),
        })
        .unwrap()
        .unwrap();
    let set_mode = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(prompt(&mut runtime, SessionAccessMode::AcceptEdits).is_err());
    runtime
        .ingest(
            &frame(json!({"method":"session/update","params":{"sessionId":"claude-session-1",
                "update":{"sessionUpdate":"config_option_update","configOptions":option("acceptEdits")}}})),
            &Mapper,
        )
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"jsonrpc":"2.0","id":set_mode,"result":{}})),
            &Mapper,
        )
        .unwrap();
    assert!(prompt(&mut runtime, SessionAccessMode::AcceptEdits).is_ok());
}
