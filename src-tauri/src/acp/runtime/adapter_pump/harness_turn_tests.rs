use super::*;

/// The exact session identity a harness names a Turn it started with.
fn session_identity() -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        session_id: session("one"),
        external_id: ExternalId::new("session", "external-session").unwrap(),
        turn_id: None,
        turn_client_message_id: None,
    }
}

fn started() -> AdapterBatch {
    owned(
        vec![AdapterEvent::TurnStarted {
            id: session_identity(),
        }],
        Vec::new(),
    )
}

fn next_item(external: &str) -> AdapterBatch {
    let next = next_turn();
    owned(
        vec![AdapterEvent::Item {
            id: AdapterIdentity {
                turn_id: next.external_turn_id,
                turn_client_message_id: Some(next.client_message_id),
                ..identity(external)
            },
            lifecycle: ItemLifecycle::Streaming,
            tool_state: None,
            payload: text(external),
        }],
        Vec::new(),
    )
}

fn durable_items(shared: &Shared) -> Vec<String> {
    shared
        .0
        .lock()
        .unwrap()
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::Item {
                external_item_id, ..
            } => Some(external_item_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_turn_the_harness_starts_after_the_terminal_keeps_its_stdout_for_the_next_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            next_item("first"),
            next_item("second"),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n{}\n", 2)
            .await
            .unwrap();
        assert!(pump.next_turn_started());
        assert!(!pump.drain_stdout_deferred(3).await.unwrap());
        assert!(
            !pump.has_pending_stdout_input(),
            "the second frame is the next turn's to read"
        );
        assert!(pump.completed_turn_ready());

        let parts = pump.try_into_completed_parts().ok().unwrap();
        assert!(parts.turn_started());
        let (parts, _) = *parts
            .bind_turn(next_turn())
            .err()
            .expect("a prompt cannot take the started Turn's place");
        let mut next = parts.bind_harness_turn(next_turn()).ok().unwrap();
        next.adopt_started_turn(4).await.unwrap();
        assert!(next.drain_stdout_deferred(5).await.unwrap());
        next.flush_deferred().await.unwrap();

        assert_eq!(durable_items(&shared), ["first", "second"]);
        assert!(!next.teardown_required());
    });
}

#[test]
fn a_turn_the_harness_starts_while_idle_is_bound_with_the_stdout_after_it() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            next_item("first"),
            next_item("second"),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        parts
            .accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut transport)
            .await
            .unwrap();
        assert!(parts.turn_started());
        assert!(
            parts
                .accept_idle_stdout_chunk(b"{}\n", 3, &mut transport)
                .await
                .is_err(),
            "nothing more is read before the Turn opens"
        );

        let mut next = parts.bind_harness_turn(next_turn()).ok().unwrap();
        next.adopt_started_turn(4).await.unwrap();
        assert!(next.drain_stdout_deferred(5).await.unwrap());
        assert!(
            !next.drain_stdout_deferred(6).await.unwrap(),
            "the refused chunk was never buffered"
        );
        next.flush_deferred().await.unwrap();

        assert_eq!(durable_items(&shared), ["first", "second"]);
        assert!(transport.0.is_empty());
    });
}

#[test]
fn a_run_the_harness_starts_before_this_turns_prompt_reaches_it_is_declined() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![started(), owned(vec![item("own")], Vec::new())]);
        pump.push_stdout_chunk_deferred(b"{}\n{}\n", 1)
            .await
            .unwrap();
        assert!(pump.drain_stdout_deferred(2).await.unwrap());
        pump.flush_deferred().await.unwrap();
        assert!(!pump.next_turn_started());
        assert!(!pump.teardown_required());
        assert_eq!(
            shared.0.lock().unwrap().log,
            ["decline", "journal", "publish"]
        );
        assert_eq!(
            pump.egress.data_usage().0,
            1,
            "the interrupt waits to be written"
        );
        assert_eq!(durable_items(&shared), ["own"]);
    });
}

#[test]
fn a_declined_turn_reads_on_through_the_stdout_after_it() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            owned(vec![AdapterEvent::Absorbed], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        parts
            .accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut transport)
            .await
            .unwrap();
        assert!(parts.turn_started());

        parts.decline_started_turn(3, &mut transport).await.unwrap();
        assert!(!parts.turn_started());
        assert_eq!(transport.0, br#"{"interrupt":true}"#);
        assert!(
            shared.0.lock().unwrap().batches.is_empty(),
            "the frame after it was read"
        );
        assert!(parts.bind_turn(next_turn()).is_ok(), "a prompt may follow");
    });
}

#[test]
fn a_refusal_read_after_a_declined_turn_is_reported_and_the_session_reads_on() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            owned(vec![AdapterEvent::Absorbed], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        parts
            .accept_idle_stdout_chunk(b"{}\n{}\n{}\n", 2, &mut transport)
            .await
            .unwrap();
        shared.0.lock().unwrap().ingest_error = Some(AdapterRuntimeError::ConfigMutationRejected);

        assert!(matches!(
            parts.decline_started_turn(3, &mut transport).await,
            Err(AdapterPumpError::Refused(NonfatalRefusal::ConfigMutation))
        ));
        assert!(!parts.turn_started());
        assert!(
            shared.0.lock().unwrap().batches.is_empty(),
            "the frame after the refusal was read"
        );
        assert!(parts.bind_turn(next_turn()).is_ok(), "a prompt may follow");
    });
}

#[test]
fn a_started_turn_is_not_bound_while_a_setting_changes() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n", 2).await.unwrap();
        assert!(pump.next_turn_started());
        let parts = pump.try_into_completed_parts().ok().unwrap();
        shared.0.lock().unwrap().config_pending = true;
        let (parts, _) = *parts.bind_harness_turn(next_turn()).err().unwrap();
        shared.0.lock().unwrap().config_pending = false;
        assert!(parts.bind_harness_turn(next_turn()).is_ok());
    });
}

/// A Turn Codex starts on its own after the last one ended, which Sage
/// refuses (here: the chat's access changed since). Codex cannot decline it,
/// so the session stops, but the chat is told why before anything else the
/// refused Turn sends is read, and no prompt binds to it meanwhile.
#[test]
fn a_turn_codex_starts_that_sage_refuses_is_shown_before_the_session_fails_closed() {
    use crate::acp::bindings::adapter::HarnessTurnAuthority;
    use sage_acp_protocol::access_mode::SessionAccessMode;
    // Recorded order when a `turn/start` meets a Turn Codex is starting:
    // `turn/started` first, then the reply naming that same Turn.
    const RACED: &str = "01a210c8-5f47-7bb3-a068-4d11d6174741";
    let line = |value: serde_json::Value| codex_line(value);
    let started = |id: &str| {
        line(
            serde_json::json!({"method":"turn/started","params":{"threadId":"thread-1",
            "turn":{"id":id,"rootTurnId":id,"items":[],"itemsView":"notLoaded",
            "status":"inProgress","error":null}}}),
        )
    };
    let ended = |id: &str, status: &str| {
        line(
            serde_json::json!({"method":"turn/completed","params":{"threadId":"thread-1",
            "turn":{"id":id,"status":status}}}),
        )
    };
    runtime().block_on(async {
        let mut codex = ready_codex();
        let prompt = codex
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "Write the fix").unwrap(),
                client_message_id: RequiredText::new("client", "client-1").unwrap(),
                access_mode: SessionAccessMode::WorkspaceWrite,
            })
            .unwrap();
        let prompt_id = codex_request_id(&prompt);
        codex
            .resolve_egress(prompt.into_token(), EgressResolution::Queued)
            .unwrap();
        let (mut pump, shared) = codex_pump(codex);
        for frame in [
            started(RACED),
            line(
                serde_json::json!({"id":prompt_id,"result":{"turn":{"id":RACED,
                "rootTurnId":RACED,"items":[],"itemsView":"notLoaded",
                "status":"inProgress","error":null}}}),
            ),
            ended(RACED, "completed"),
        ] {
            pump.push_stdout_chunk(&frame, 2).await.unwrap();
        }
        let parts = pump
            .try_into_completed_parts()
            .ok()
            .expect("the prompt owns the raced Turn");

        // A prompt Codex refuses changes nothing the thread keeps.
        let next = ActiveTurnIntent {
            turn_id: turn("two"),
            external_turn_id: None,
            client_message_id: RequiredText::new("client", "client-2").unwrap(),
        };
        let mut pump = parts
            .bind_turn(next.clone())
            .ok()
            .expect("a prompt may follow");
        pump.queue_prompt(
            Prompt::new("prompt", "Look only").unwrap(),
            &next.client_message_id,
            SessionAccessMode::ReadOnly,
        )
        .unwrap();
        let mut transport = CaptureTransport(Vec::new());
        assert_eq!(
            pump.flush_one(&mut transport).unwrap(),
            FlushDisposition::Written
        );
        let refused =
            serde_json::from_slice::<serde_json::Value>(&transport.0).unwrap()["id"].clone();
        pump.push_stdout_chunk(
            &line(serde_json::json!({"id":refused,"error":{"code":-32600,"message":"rejected"}})),
            3,
        )
        .await
        .unwrap();
        pump.push_stdout_chunk(&started("native-1"), 4)
            .await
            .unwrap();
        assert!(pump.next_turn_started());
        let mut parts = pump
            .try_into_completed_parts()
            .ok()
            .expect("the failed prompt ends");
        assert!(parts.turn_started());
        assert_eq!(
            parts.harness_turn_authority(),
            Some(HarnessTurnAuthority::Access(
                SessionAccessMode::WorkspaceWrite
            ))
        );

        let notices = |shared: &Shared| {
            shared
                .0
                .lock()
                .unwrap()
                .durable_batches
                .iter()
                .flatten()
                .filter(|event| matches!(
                    event,
                    DurableAdapterEvent::Session {
                        event: sage_acp_protocol::intent::session_event::SessionScopedEvent::Notice(notice),
                        ..
                    } if notice.level == sage_acp_protocol::intent::session_event::SessionNoticeLevel::Error
                ))
                .count()
        };
        let mut transport = CaptureTransport(Vec::new());
        assert!(
            parts.decline_started_turn(5, &mut transport).await.is_err(),
            "the runtime fails closed"
        );
        assert!(transport.0.is_empty(), "nothing is written for the refused Turn");
        assert_eq!(notices(&shared), 1, "the chat is told why the session stops");
        // Until the runtime is gone, no prompt binds to the refused Turn, and
        // a request the refused Turn sends adds no second, silent failure.
        let Err(refused) = parts.bind_turn(next_turn()) else {
            panic!("a prompt bound to the refused Turn");
        };
        let (mut parts, _) = *refused;
        let approval = parts
            .accept_idle_stdout_chunk(
                &line(serde_json::json!({"id":90,"method":"item/commandExecution/requestApproval",
                    "params":{"threadId":"thread-1","turnId":"native-1","itemId":"cmd-1",
                    "command":"rm -rf /","cwd":"C:\\repo"}})),
                6,
                &mut transport,
            )
            .await;
        assert!(matches!(approval, Err(AdapterPumpError::Intent)), "{approval:?}");
        assert_eq!(notices(&shared), 1);
    });
}

/// Recorded with codex-cli 0.167.1: a read-only prompt sent as Codex starts
/// a goal continuation on a thread last prompted under workspace-write is
/// taken into that Turn, which keeps workspace-write and wrote to the
/// workspace. The `turn/started` that arrives before Codex answers the
/// prompt is refused, so the prompt never runs under the wider access.
/// A `/compact` or `/review` raced the same way gets the goal Turn's
/// `turn/started` before its answer, so it is refused too. A compact
/// answered first binds its own `turn/started`, whatever the thread's
/// access, and an answered review leaves the thread's access as it was.
#[test]
fn a_narrower_prompt_codex_takes_into_its_own_wider_turn_is_refused() {
    use crate::acp::bindings::adapter::{AdapterRuntimeError, HarnessTurnAuthority};
    use crate::acp::session_action::{ReviewTarget, SessionAction};
    use sage_acp_protocol::access_mode::SessionAccessMode;
    const FIRST: &str = "01a2152a-48c9-7ab1-8af4-951391e0316a";
    const GOAL: &str = "01a2152a-5858-7b42-8c45-f03f0fd8f5b6";
    const REVIEW: &str = "01a2155e-a7cf-77e1-bf7a-4525903a99e8";
    let turn_object = |id: &str| {
        serde_json::json!({"id":id,"rootTurnId":id,"items":[],"itemsView":"notLoaded",
            "status":"inProgress","error":null,"startedAt":null,"completedAt":null,
            "durationMs":null})
    };
    let started = |id: &str| {
        codex_line(serde_json::json!({"method":"turn/started","params":{
            "threadId":"thread-1","turn":{"id":id,"rootTurnId":id,"items":[],
            "itemsView":"notLoaded","status":"inProgress","error":null,
            "startedAt":1795651426,"completedAt":null,"durationMs":null}}}))
    };
    let completed = |id: &str| {
        codex_line(serde_json::json!({"method":"turn/completed","params":{
            "threadId":"thread-1","turn":{"id":id,"status":"completed"}}}))
    };
    let joined = |action: Option<SessionAction>| {
        let Some(action) = action else {
            return Prompt::new("prompt", "Reply with the word JOINED as well.").unwrap();
        };
        let mut prompt = Prompt::new("prompt", action.prompt_text()).unwrap();
        prompt.action = Some(action);
        prompt
    };
    let compact = || Some(SessionAction::Compact {});
    let review = || {
        Some(SessionAction::Review {
            target: ReviewTarget::UncommittedChanges {},
        })
    };
    runtime().block_on(async {
        for (action, answered_first) in [
            (None, false),
            (compact(), false),
            (review(), false),
            (compact(), true),
            (review(), true),
        ] {
            let is_review = matches!(action, Some(SessionAction::Review { .. }));
            let mut codex = ready_codex();
            let prompt = codex
                .command(AdapterCommand::Prompt {
                    prompt: Prompt::new("prompt", "Reply with exactly the word READY.").unwrap(),
                    client_message_id: RequiredText::new("client", "client-1").unwrap(),
                    access_mode: SessionAccessMode::WorkspaceWrite,
                })
                .unwrap();
            let prompt_id = codex_request_id(&prompt);
            codex
                .resolve_egress(prompt.into_token(), EgressResolution::Queued)
                .unwrap();
            let (mut pump, shared) = codex_pump(codex);
            for frame in [
                codex_line(
                    serde_json::json!({"id":prompt_id,"result":{"turn":turn_object(FIRST)}}),
                ),
                started(FIRST),
                completed(FIRST),
            ] {
                pump.push_stdout_chunk(&frame, 2).await.unwrap();
            }
            let parts = pump
                .try_into_completed_parts()
                .ok()
                .expect("the workspace-write prompt ends");

            let next = ActiveTurnIntent {
                turn_id: turn("two"),
                external_turn_id: None,
                client_message_id: RequiredText::new("client", "client-2").unwrap(),
            };
            let mut pump = parts
                .bind_turn(next.clone())
                .ok()
                .expect("a prompt may follow");
            pump.queue_prompt(
                joined(action),
                &next.client_message_id,
                SessionAccessMode::ReadOnly,
            )
            .unwrap();
            let mut transport = CaptureTransport(Vec::new());
            assert_eq!(
                pump.flush_one(&mut transport).unwrap(),
                FlushDisposition::Written
            );
            if !answered_first {
                let refused = pump.push_stdout_chunk(&started(GOAL), 3).await;
                assert!(
                    matches!(
                        refused,
                        Err(AdapterPumpError::Adapter(
                            AdapterRuntimeError::HarnessTurnRefused
                        ))
                    ),
                    "review={is_review}: {refused:?}"
                );
                continue;
            }
            let id = serde_json::from_slice::<serde_json::Value>(&transport.0).unwrap()["id"]
                .as_u64()
                .unwrap();
            if !is_review {
                let answer = codex_line(serde_json::json!({"id":id,"result":{}}));
                pump.push_stdout_chunk(&answer, 3).await.unwrap();
                pump.push_stdout_chunk(&started(GOAL), 3).await.unwrap();
                let bound = shared
                    .0
                    .lock()
                    .unwrap()
                    .durable_batches
                    .iter()
                    .flatten()
                    .any(|event| {
                        matches!(event, DurableAdapterEvent::Turn {
                            scope, native_turn_id: Some(native), ..
                        } if scope.turn_id == next.turn_id && native.as_str() == GOAL)
                    });
                assert!(bound, "the compact's own Turn binds to the chat's Turn");
                continue;
            }
            let answer = codex_line(
                serde_json::json!({"id":id,"result":{"turn":turn_object(REVIEW),
                    "reviewThreadId":"thread-1"}}),
            );
            for frame in [answer, started(REVIEW), completed(REVIEW)] {
                pump.push_stdout_chunk(&frame, 3).await.unwrap();
            }
            let mut parts = pump
                .try_into_completed_parts()
                .ok()
                .expect("the review ends");
            parts
                .accept_idle_stdout_chunk(&started(GOAL), 4, &mut transport)
                .await
                .unwrap();
            assert!(parts.turn_started());
            assert_eq!(
                parts.harness_turn_authority(),
                Some(HarnessTurnAuthority::Access(
                    SessionAccessMode::WorkspaceWrite
                )),
                "a review sets no policy; Codex keeps the wider access"
            );
        }
    });
}
