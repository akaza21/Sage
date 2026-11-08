//! The `codex-user-input` native replay journey, frame for frame, through
//! the real Codex binding and the adapter pump: both turns ask, both
//! questions reach the chat for their own turn, and both are answered
//! without tearing the session down.

use super::*;
use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};

const ANSWER: &str = "Changed files";

fn fixture_frames() -> Vec<serde_json::Value> {
    crate::acp::fixture::codex_user_input_wire_for_test(&serde_json::json!({
        "answers":{"scope":{"answers":[ANSWER]},"token":{"answers":["user_note: secret"]}}
    }))
}

/// Codex initialized with the fixture's thread and its goal probe refused,
/// as the native journey opens, holding the first prompt.
fn opened_codex(frames: &[serde_json::Value]) -> (CodexRuntime, u64) {
    let mut codex = CodexRuntime::fixture(Platform::WindowsX86_64);
    let init = codex.command(AdapterCommand::Initialize).unwrap();
    codex
        .resolve_egress(init.into_token(), EgressResolution::Queued)
        .unwrap();
    let initialized = codex
        .ingest(
            &codex_frame(serde_json::json!({"id":1,"result":{
                "userAgent":user_agent("sage", "unknown"),
                "codexHome":MANAGED_CODEX_HOME,
                "platformFamily":"windows","platformOs":"windows"
            }})),
            &Mapper,
        )
        .unwrap();
    codex
        .resolve_egress(
            initialized.egress()[0].token().clone(),
            EgressResolution::Queued,
        )
        .unwrap();
    let start = codex
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    let mut thread = frames[1].clone();
    thread["id"] = codex_request_id(&start).into();
    codex.ingest(&codex_frame(thread), &Mapper).unwrap();
    let goal = codex
        .goal_get()
        .unwrap()
        .expect("pinned Codex probes for a goal");
    let goal_id = codex_request_id(&goal);
    codex
        .resolve_egress(goal.into_token(), EgressResolution::Queued)
        .unwrap();
    let mut refused = frames[2].clone();
    refused["id"] = goal_id.into();
    codex.ingest(&codex_frame(refused), &Mapper).unwrap();
    let prompt = codex
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Ask me.").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = codex_request_id(&prompt);
    codex
        .resolve_egress(prompt.into_token(), EgressResolution::Queued)
        .unwrap();
    (codex, prompt_id)
}

/// Plays one fixture turn: its frames up to the question, the user's
/// answer, then the rest of the turn.
async fn play_turn(
    mut pump: AdapterPump<CodexRuntime, FakeJournal, Mapper>,
    shared: &Shared,
    turn: &[serde_json::Value],
    prompt_id: u64,
) -> CompletedPumpParts<CodexRuntime, FakeJournal, Mapper> {
    let ask = turn
        .iter()
        .position(|frame| frame["method"] == "item/tool/requestUserInput")
        .expect("every turn of the journey asks");
    for (index, frame) in turn.iter().enumerate() {
        let mut frame = frame.clone();
        if frame.pointer("/result/turn").is_some() {
            frame["id"] = prompt_id.into();
        }
        pump.push_stdout_chunk(&codex_line(frame), 2)
            .await
            .unwrap_or_else(|error| panic!("frame {index} of the turn failed: {error:?}"));
        if index != ask {
            continue;
        }
        let request = format!("number:{}", turn[ask]["id"]);
        let journaled = shared
            .0
            .lock()
            .unwrap()
            .durable_batches
            .iter()
            .flatten()
            .filter_map(|event| match event {
                DurableAdapterEvent::PermissionRequest { intent, .. }
                    if intent.external_request_id.as_str() == request =>
                {
                    Some((intent.request_id.clone(), intent.turn_id.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let [(sage_request, sage_turn)] = journaled.as_slice() else {
            panic!("question {request} must reach the chat once, got {journaled:?}");
        };
        assert_eq!(sage_turn, &pump.intent.active_turn.turn_id);
        let answers = serde_json::from_value(serde_json::json!([
            {"questionId":"scope","selectedOption":ANSWER},
            {"questionId":"token","text":"secret"}
        ]))
        .unwrap();
        pump.resolve_permission(
            sage_request,
            crate::acp::runtime::permissions::PermissionDecision::Answer(answers),
            3,
        )
        .await
        .unwrap();
        let mut transport = CaptureTransport(Vec::new());
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        let reply: serde_json::Value = serde_json::from_slice(&transport.0).unwrap();
        assert_eq!(reply["id"], turn[ask]["id"]);
        assert_eq!(
            reply["result"]["answers"]["scope"]["answers"],
            serde_json::json!([ANSWER])
        );
        assert!(pump.settle_written_permission(4).await.unwrap());
    }
    assert!(!pump.teardown_required());
    pump.try_into_completed_parts()
        .ok()
        .expect("the turn completes and hands the session on")
}

#[test]
fn the_codex_user_input_journey_asks_and_is_answered_on_both_turns() {
    runtime().block_on(async {
        let frames = fixture_frames();
        let turn_starts = frames
            .iter()
            .enumerate()
            .filter(|(_, frame)| frame.pointer("/result/turn").is_some())
            .map(|(index, _)| index)
            .collect::<Vec<_>>();
        let [first, second] = turn_starts.as_slice() else {
            panic!("the journey runs two turns");
        };
        let (codex, prompt_id) = opened_codex(&frames);
        let (pump, shared) = codex_pump(codex);
        let parts = play_turn(pump, &shared, &frames[*first..*second], prompt_id).await;

        let client = RequiredText::new("client", "client-2").unwrap();
        let mut pump = parts
            .bind_turn(ActiveTurnIntent {
                turn_id: turn("two"),
                external_turn_id: None,
                client_message_id: client.clone(),
            })
            .ok()
            .expect("the session binds its second turn");
        pump.queue_prompt(
            Prompt::new("prompt", "Ask again.").unwrap(),
            &client,
            crate::settings_store::SessionAccessMode::ReadOnly,
        )
        .unwrap();
        let mut transport = CaptureTransport(Vec::new());
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        let prompt_id = serde_json::from_slice::<serde_json::Value>(&transport.0).unwrap()["id"]
            .as_u64()
            .unwrap();
        let rest = &frames[*second..];
        let end = rest
            .iter()
            .position(|frame| frame["method"] == "turn/completed")
            .expect("the second turn completes");
        play_turn(pump, &shared, &rest[..=end], prompt_id).await;
    });
}
