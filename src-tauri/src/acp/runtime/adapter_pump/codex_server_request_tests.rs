//! Codex server requests that wait for the user (approvals and questions)
//! must reach the durable journal as permission rows through the real
//! Codex binding, so the chat can show the card and the harness gets an
//! answer.

use super::*;

async fn running_codex_turn() -> (AdapterPump<CodexRuntime, FakeJournal, Mapper>, Shared) {
    let (mut pump, shared, prompt_id) = goal_active_codex_pump();
    start_turn(&mut pump, prompt_id, "turn-1", "client-1").await;
    assert!(
        shared
            .0
            .lock()
            .unwrap()
            .durable_batches
            .iter()
            .flatten()
            .any(|event| matches!(
                event,
                DurableAdapterEvent::Turn {
                    state: TurnState::Running,
                    ..
                }
            )),
        "the turn must be running before the harness asks the user"
    );
    (pump, shared)
}

/// Codex's answer to the prompt, then the turn starting and echoing the
/// user's message, which binds it to the pump's active turn.
async fn start_turn(
    pump: &mut AdapterPump<CodexRuntime, FakeJournal, Mapper>,
    prompt_id: u64,
    turn: &str,
    client: &str,
) {
    for frame in [
        serde_json::json!({"id":prompt_id,"result":{"turn":{"id":turn}}}),
        serde_json::json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":turn}}}),
        serde_json::json!({"method":"item/started","params":{"threadId":"thread-1","turnId":turn,
            "item":{"id":format!("echo-{turn}"),"type":"userMessage","clientId":client,
                "text":"Inspect only"}}}),
    ] {
        assert!(pump.push_stdout_chunk(&codex_line(frame), 1).await.unwrap());
    }
}

/// Each journaled request as its native id and whether it carries questions.
fn permission_requests(shared: &Shared) -> Vec<(String, Option<bool>)> {
    shared
        .0
        .lock()
        .unwrap()
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::PermissionRequest { intent, .. } => Some((
                intent.external_request_id.as_str().to_owned(),
                match serde_json::from_str(&intent.normalized_payload_json) {
                    Ok(NormalizedPayload::PermissionRequest { user_input, .. }) => {
                        Some(user_input.is_some())
                    }
                    _ => None,
                },
            )),
            _ => None,
        })
        .collect()
}

#[test]
fn codex_command_approval_reaches_the_journal_for_the_running_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = running_codex_turn().await;
        assert!(
            pump.push_stdout_chunk(
                &codex_line(serde_json::json!({
                    "id":41,"method":"item/commandExecution/requestApproval","params":{
                        "threadId":"thread-1","turnId":"turn-1","itemId":"cmd-1","command":"ls"}
                })),
                2
            )
            .await
            .unwrap()
        );
        assert_eq!(
            permission_requests(&shared),
            [("number:41".to_owned(), Some(false))]
        );
    });
}

#[test]
fn codex_question_reaches_the_journal_for_the_running_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = running_codex_turn().await;
        assert!(
            pump.push_stdout_chunk(
                &codex_line(serde_json::json!({
                    "id":42,"method":"item/tool/requestUserInput","params":{
                        "threadId":"thread-1","turnId":"turn-1","itemId":"call-1",
                        "questions":[{"id":"scope","header":"Scope","question":"Which files?",
                            "isOther":false,"isSecret":false,
                            "options":[{"label":"All","description":"Every file"}]}]}
                })),
                2
            )
            .await
            .unwrap()
        );
        assert_eq!(
            permission_requests(&shared),
            [("number:42".to_owned(), Some(true))]
        );
    });
}

#[test]
fn codex_withdrawing_a_question_resolves_the_journaled_request() {
    runtime().block_on(async {
        let (mut pump, shared) = running_codex_turn().await;
        for frame in [
            serde_json::json!({"id":43,"method":"item/tool/requestUserInput","params":{
                "threadId":"thread-1","turnId":"turn-1","itemId":"call-1",
                "questions":[{"id":"scope","header":"Scope","question":"Which files?",
                    "isOther":true,"isSecret":false,"options":null}]}}),
            serde_json::json!({"method":"serverRequest/resolved","params":{
                "threadId":"thread-1","requestId":43}}),
        ] {
            assert!(pump.push_stdout_chunk(&codex_line(frame), 2).await.unwrap());
        }
        let resolved = shared
            .0
            .lock()
            .unwrap()
            .durable_batches
            .iter()
            .flatten()
            .filter_map(|event| match event {
                DurableAdapterEvent::PermissionResolved { state, .. } => Some(*state),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(resolved, [ServerRequestState::Cancelled]);
    });
}

/// A Codex server request that waits for the user, and how the user answers.
struct Ask {
    method: &'static str,
    params: serde_json::Value,
    decision: fn() -> crate::acp::runtime::permissions::PermissionDecision,
    reply: serde_json::Value,
}

fn question() -> Ask {
    Ask {
        method: "item/tool/requestUserInput",
        params: serde_json::json!({"questions":[{"id":"scope","header":"Scope",
            "question":"Which files?","isOther":true,"isSecret":false,"options":null}]}),
        decision: || {
            crate::acp::runtime::permissions::PermissionDecision::Answer(
                serde_json::from_value(serde_json::json!([
                    {"questionId":"scope","text":"Changed files"}
                ]))
                .unwrap(),
            )
        },
        reply: serde_json::json!({"answers":{"scope":{"answers":["user_note: Changed files"]}}}),
    }
}

fn command_approval() -> Ask {
    Ask {
        method: "item/commandExecution/requestApproval",
        params: serde_json::json!({"command":"ls"}),
        decision: || crate::acp::runtime::permissions::PermissionDecision::AllowOnce,
        reply: serde_json::json!({"decision":"accept"}),
    }
}

/// Codex asks on a turn, the user answers, Codex reads the reply and the
/// turn completes. Returns the pump ready for the next turn.
async fn ask_answer_and_complete(
    mut pump: AdapterPump<CodexRuntime, FakeJournal, Mapper>,
    shared: &Shared,
    ask: &Ask,
    request_id: u64,
    turn: &str,
) -> CompletedPumpParts<CodexRuntime, FakeJournal, Mapper> {
    let mut params = ask.params.clone();
    params["threadId"] = serde_json::json!("thread-1");
    params["turnId"] = serde_json::json!(turn);
    params["itemId"] = serde_json::json!(format!("item-{request_id}"));
    assert!(
        pump.push_stdout_chunk(
            &codex_line(serde_json::json!({
                "id":request_id,"method":ask.method,"params":params
            })),
            2
        )
        .await
        .unwrap()
    );
    let journaled = shared
        .0
        .lock()
        .unwrap()
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::PermissionRequest { intent, .. }
                if intent.external_request_id.as_str() == format!("number:{request_id}") =>
            {
                Some((intent.request_id.clone(), intent.turn_id.clone()))
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    let [(sage_request, sage_turn)] = journaled.as_slice() else {
        panic!("request {request_id} on {turn} must be journaled once, got {journaled:?}");
    };
    assert_eq!(sage_turn, &pump.intent.active_turn.turn_id);
    pump.resolve_permission(sage_request, (ask.decision)(), 3)
        .await
        .unwrap();
    let mut transport = CaptureTransport(Vec::new());
    assert_eq!(
        pump.flush_one(&mut transport).unwrap(),
        FlushDisposition::Written
    );
    let reply: serde_json::Value = serde_json::from_slice(&transport.0).unwrap();
    assert_eq!(reply["id"], request_id);
    assert_eq!(reply["result"], ask.reply);
    assert!(pump.settle_written_permission(4).await.unwrap());
    assert!(pump.push_stdout_chunk(&codex_line(serde_json::json!({
        "method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":turn,"status":"completed"}}
    })), 5).await.unwrap());
    assert!(!pump.teardown_required());
    pump.try_into_completed_parts()
        .ok()
        .expect("the turn hands the session on")
}

/// Two turns in one session each ask, and each request reaches the chat
/// for its own turn and gets answered, with no session teardown.
fn asks_on_two_turns(ask: Ask) {
    runtime().block_on(async {
        let (pump, shared) = running_codex_turn().await;
        let parts = ask_answer_and_complete(pump, &shared, &ask, 41, "turn-1").await;
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
        start_turn(&mut pump, prompt_id, "turn-2", "client-2").await;
        ask_answer_and_complete(pump, &shared, &ask, 42, "turn-2").await;
    });
}

#[test]
fn codex_questions_on_two_turns_are_each_shown_and_answered() {
    asks_on_two_turns(question());
}

#[test]
fn codex_approvals_on_two_turns_are_each_shown_and_answered() {
    asks_on_two_turns(command_approval());
}

#[test]
fn a_late_codex_request_for_a_finished_turn_is_declined_not_left_waiting() {
    runtime().block_on(async {
        let (pump, shared) = running_codex_turn().await;
        let parts = ask_answer_and_complete(pump, &shared, &command_approval(), 41, "turn-1").await;
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
            Prompt::new("prompt", "Again.").unwrap(),
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
        start_turn(&mut pump, prompt_id, "turn-2", "client-2").await;
        assert!(
            pump.push_stdout_chunk(
                &codex_line(serde_json::json!({
                    "id":43,"method":"item/commandExecution/requestApproval","params":{
                        "threadId":"thread-1","turnId":"turn-1","itemId":"late-1","command":"ls"}
                })),
                6
            )
            .await
            .unwrap()
        );
        assert!(
            !permission_requests(&shared)
                .iter()
                .any(|(id, _)| id == "number:43")
        );
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&transport.0).unwrap(),
            serde_json::json!({"id":43,"result":{"decision":"decline"}})
        );
        assert!(!pump.teardown_required());
    });
}
