//! Frames recorded from codex-cli 0.167.1 (CDX-AUTO-TURN probe): a thread
//! whose goal was still active, resumed by a new app-server. About 250 ms
//! after `thread/resume` Codex starts a Turn nobody asked for, which ends
//! failed under the account's usage limit.
use super::*;
use crate::acp::bindings::adapter::HarnessTurnAuthority;
use sage_acp_protocol::{access_mode::SessionAccessMode, lifecycle::TurnState};

const THREAD: &str = "01a210c4-72f3-7480-860b-4539c4ec1a60";
const TURN: &str = "01a210c5-2b48-7221-9df3-cec4b7b13243";
const LIMIT: &str = "You've hit your usage limit. Upgrade to Pro (https://chatgpt.com/explore/pro), visit https://chatgpt.com/settings/usage to purchase more credits or try again at Nov 29th, 2026 1:13 PM.";

fn notification(method: &str, params: Value) -> Frame {
    frame(json!({"method": method, "params": params}))
}

fn goal(status: &str, turn: Value) -> Frame {
    notification(
        "thread/goal/updated",
        json!({"threadId":THREAD,"turnId":turn,"goal":{"threadId":THREAD,
            "objective":"probe objective","status":status,"tokenBudget":null,"tokensUsed":0,
            "timeUsedSeconds":12,"createdAt":1795577639,"updatedAt":1795577692}}),
    )
}

fn turn(status: &str, error: Value) -> Value {
    json!({"id":TURN,"rootTurnId":TURN,"items":[],"itemsView":"notLoaded","status":status,
        "error":error,"startedAt":1795577686,"completedAt":null,"durationMs":null})
}

fn status(kind: Value) -> Frame {
    notification(
        "thread/status/changed",
        json!({"threadId":THREAD,"status":kind,"emittedAtMs":1795577686875_u64}),
    )
}

/// Resumes the thread, then reads what Codex sent up to its own Turn's start.
fn resumed_until_codex_starts_a_turn() -> (CodexRuntime, AdapterBatch) {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let resume = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("thread", THREAD).unwrap(),
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    for frame in [
        status(json!({"type":"idle"})),
        frame(json!({"id":request_id(&resume),"result":{
            "thread":{"id":THREAD,"status":{"type":"idle"}},
            "model":"gpt-6.1-sol","reasoningEffort":"high"
        }})),
        goal("active", Value::Null),
        status(json!({"type":"active","activeFlags":[]})),
    ] {
        runtime.ingest(&frame, &Mapper).unwrap();
    }
    let started = runtime
        .ingest(
            &notification(
                "turn/started",
                json!({"threadId":THREAD,"turn":turn("inProgress", Value::Null)}),
            ),
            &Mapper,
        )
        .expect("a Turn Codex starts on its own is announced, not a protocol error");
    (runtime, started)
}

fn adopt(runtime: &mut CodexRuntime) -> AdapterBatch {
    runtime
        .adopt_turn(RequiredText::new("client", "sage-turn-1").unwrap(), &Mapper)
        .unwrap()
}

fn names_adopted_turn(id: &AdapterIdentity) -> bool {
    id.turn_id.as_ref().map(ExternalId::as_str) == Some(TURN)
        && id.turn_client_message_id.as_ref().map(RequiredText::as_str) == Some("sage-turn-1")
}

#[test]
fn a_turn_codex_starts_after_resume_runs_as_an_adopted_turn_through_its_failed_end() {
    let (mut runtime, started) = resumed_until_codex_starts_a_turn();
    let [AdapterEvent::TurnStarted { id }] = started.events() else {
        panic!("expected only the announcement, got {:?}", started.events());
    };
    assert_eq!(id.external_id.as_str(), THREAD);
    assert!(id.turn_id.is_none() && id.turn_client_message_id.is_none());
    assert_eq!(
        runtime.harness_turn_authority(),
        Some(HarnessTurnAuthority::Access(SessionAccessMode::ReadOnly)),
        "a resumed thread runs read-only until a prompt sets its access"
    );

    let opened = adopt(&mut runtime);
    assert!(matches!(
        opened.events(),
        [AdapterEvent::Turn { id, state: TurnState::Running }] if names_adopted_turn(id)
    ));
    assert_eq!(runtime.harness_turn_authority(), None);

    for frame in [
        goal("active", json!(TURN)),
        goal("usageLimited", json!(TURN)),
        status(json!({"type":"systemError"})),
    ] {
        runtime.ingest(&frame, &Mapper).unwrap();
    }
    let error = runtime
        .ingest(
            &notification(
                "error",
                json!({"error":{"message":LIMIT,"codexErrorInfo":"usageLimitExceeded",
                    "additionalDetails":null,"misalignment":null},
                    "willRetry":false,"threadId":THREAD,"turnId":TURN}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(error.events(), [AdapterEvent::Error(_)]));
    let ended = runtime
        .ingest(
            &notification(
                "turn/completed",
                json!({"threadId":THREAD,"turn":turn("failed", json!({"message":LIMIT,
                    "codexErrorInfo":"usageLimitExceeded","additionalDetails":null,
                    "misalignment":null}))}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(ended.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Turn { id, state: TurnState::Failed } if names_adopted_turn(id)
    )));
}

#[test]
fn stop_interrupts_the_adopted_turn_and_a_prompt_sets_the_access_codex_keeps() {
    let (mut runtime, _) = resumed_until_codex_starts_a_turn();
    adopt(&mut runtime);
    let stop = runtime.command(AdapterCommand::Cancel).unwrap();
    let wire: Value = serde_json::from_slice(stop.frame().payload()).unwrap();
    assert_eq!(wire["method"], "turn/interrupt");
    assert_eq!(wire["params"]["turnId"], TURN);
    let stop_id = request_id(&stop);
    runtime
        .resolve_egress(stop.into_token(), EgressResolution::Written)
        .unwrap();
    runtime
        .ingest(&frame(json!({"id":stop_id,"result":{}})), &Mapper)
        .unwrap();
    let interrupted = runtime
        .ingest(
            &notification(
                "turn/completed",
                json!({"threadId":THREAD,"turn":turn("interrupted", Value::Null)}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(interrupted.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Turn { id, state: TurnState::Cancelled } if names_adopted_turn(id)
    )));

    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Write the fix").unwrap(),
            client_message_id: RequiredText::new("client", "sage-turn-2").unwrap(),
            access_mode: SessionAccessMode::WorkspaceWrite,
        })
        .unwrap();
    for frame in [
        frame(json!({"id":request_id(&prompt),"result":{"turn":{"id":"turn-2"}}})),
        notification(
            "turn/started",
            json!({"threadId":THREAD,"turn":{"id":"turn-2"}}),
        ),
        notification(
            "turn/completed",
            json!({"threadId":THREAD,"turn":{"id":"turn-2","status":"completed"}}),
        ),
        notification(
            "turn/started",
            json!({"threadId":THREAD,"turn":{"id":"turn-3"}}),
        ),
    ] {
        runtime.ingest(&frame, &Mapper).unwrap();
    }
    assert_eq!(
        runtime.harness_turn_authority(),
        Some(HarnessTurnAuthority::Access(
            SessionAccessMode::WorkspaceWrite
        ))
    );
}
