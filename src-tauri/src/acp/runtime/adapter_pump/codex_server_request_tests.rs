//! Codex server requests that wait for the user (approvals and questions)
//! must reach the durable journal as permission rows through the real
//! Codex binding, so the chat can show the card and the harness gets an
//! answer.

use super::*;

async fn running_codex_turn() -> (AdapterPump<CodexRuntime, FakeJournal, Mapper>, Shared) {
    let (mut pump, shared, prompt_id) = goal_active_codex_pump();
    for frame in [
        serde_json::json!({"id":prompt_id,"result":{"turn":{"id":"turn-1"}}}),
        serde_json::json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":"turn-1"}}}),
        serde_json::json!({"method":"item/started","params":{"threadId":"thread-1","turnId":"turn-1",
            "item":{"id":"echo-1","type":"userMessage","clientId":"client-1","text":"Inspect only"}}}),
    ] {
        assert!(pump.push_stdout_chunk(&codex_line(frame), 1).await.unwrap());
    }
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
