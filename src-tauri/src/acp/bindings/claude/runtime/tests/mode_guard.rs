//! A turn stays in the permission mode Sage selected for it: plan-exit
//! options that widen the mode are never picked, and a mode change the turn
//! was not given cancels it with a notice.

use super::*;
use crate::acp::{
    intent::session_event::{SessionNoticeLevel, SessionScopedEvent},
    runtime::permissions::PermissionDecision,
};
use crate::settings_store::SessionAccessMode;

const MODES: &str = r#"{"currentModeId":"plan","availableModes":[
    {"id":"default","name":"Manual"},{"id":"acceptEdits","name":"Accept edits"},
    {"id":"plan","name":"Plan"},{"id":"auto","name":"Auto"},
    {"id":"bypassPermissions","name":"Bypass permissions"}]}"#;

fn planning(runtime: &mut ClaudeRuntime, mode: SessionAccessMode) {
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

/// The 0.76 adapter's plan-exit options, in its order.
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

fn offered(batch: &AdapterBatch) -> Vec<crate::acp::intent::PermissionOptionKind> {
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
    use crate::acp::intent::PermissionOptionKind::{AllowAlways, AllowOnce};
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

    // Where accepting edits is the elevated exit, "always" picks it and
    // skips the variant that clears the conversation.
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
    // The mode that answer leads to is the turn's to take.
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
