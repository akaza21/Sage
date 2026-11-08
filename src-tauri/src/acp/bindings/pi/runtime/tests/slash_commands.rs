use super::*;

fn extension_delivery_failure_allows_retry(resolution: EgressResolution) {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(&mut runtime, query);
    runtime
        .ingest(
            &response(
                &query,
                json!({"commands":[{"name":"review","source":"extension"}]}),
            ),
            &Mapper,
        )
        .unwrap();
    let session = runtime.external_session.clone();
    let revision = runtime.available_commands().snapshot().revision;
    let command = || {
        let mut prompt = Prompt::new("prompt", "/review src").unwrap();
        prompt.command_revision = Some(revision);
        AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "retry-review").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        }
    };
    let send = runtime.command(command()).unwrap();
    let rejected_id = request(&send)["id"].clone();
    let token = send.into_token();
    if resolution == EgressResolution::WriteZeroBytes {
        runtime
            .resolve_egress(token.clone(), EgressResolution::Queued)
            .unwrap();
    }
    runtime
        .resolve_egress(token, resolution)
        .expect("undelivered extension must roll back without session teardown");

    let retry = runtime
        .command(command())
        .expect("same session and client message must accept a retry");
    let retry = deliver(&mut runtime, retry);
    assert_ne!(retry["id"], rejected_id);
    assert_eq!(retry["message"], "/review src");
    let done = runtime
        .ingest(&response(&retry, Value::Null), &Mapper)
        .unwrap();
    assert!(matches!(
        done.events(),
        [AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Completed,
            ..
        }]
    ));
    assert_eq!(runtime.external_session, session);
    running(&mut runtime);
}

#[test]
fn extension_queue_rejection_rolls_back_and_retry_completes_in_same_session() {
    extension_delivery_failure_allows_retry(EgressResolution::QueueRejected);
}

#[test]
fn extension_zero_byte_write_rolls_back_and_retry_completes_in_same_session() {
    extension_delivery_failure_allows_retry(EgressResolution::WriteZeroBytes);
}

#[test]
fn idle_idless_command_discovery_rejection_keeps_the_next_prompt_usable() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let query = runtime.discover_commands().unwrap().unwrap();
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({
                "type":"response", "command":"get_commands", "success":false,
                "error":"Unknown command: get_commands"
            })),
            &Mapper,
        )
        .expect("unsupported idle discovery must degrade to an empty catalog");
    assert!(batch.egress().is_empty());
    let catalog = runtime.available_commands().snapshot();
    assert_eq!(catalog.revision, 1);
    // Only Pi's built-in commands remain.
    assert_eq!(names(&catalog.commands), ["compact"]);
    assert!(runtime.discover_commands().unwrap().is_none());

    running(&mut runtime);
    runtime
        .ingest(&frame(json!({"type":"agent_end","messages":[]})), &Mapper)
        .unwrap();
    let batch = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap();
    assert!(batch.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Turn {
            state: crate::acp::lifecycle::TurnState::Completed,
            ..
        }
    )));
}

#[test]
fn idless_command_discovery_replies_require_one_pending_request_and_a_rejection() {
    for (pending_count, success) in [(1, true), (2, false)] {
        let mut runtime = fixture_runtime(1);
        ready(&mut runtime);
        let query = runtime.discover_commands().unwrap().unwrap();
        runtime
            .resolve_egress(query.into_token(), EgressResolution::Written)
            .unwrap();
        for _ in 1..pending_count {
            runtime.translator.begin(Operation::GetCommands).unwrap();
        }
        assert!(
            runtime
                .ingest(
                    &frame(json!({
                        "type":"response", "command":"get_commands", "success":success,
                        "data":{"commands":[{"name":"invented","source":"extension"}]}
                    })),
                    &Mapper,
                )
                .is_err()
        );
        assert_eq!(runtime.available_commands().snapshot().revision, 0);
    }
}

#[test]
fn idle_command_discovery_preserves_rpc_capacity_across_turns() {
    use crate::acp::limits::LIMITS;

    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    for turn in 0..=LIMITS.queues.outstanding_rpc_per_session {
        let egress = runtime
            .discover_commands()
            .unwrap()
            .expect("idle discovery must remain available");
        let query = request(&egress);
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .expect("idle discovery writes directly to the transport");
        assert_eq!(query["type"], "get_commands");
        runtime
            .ingest(
                &response(
                    &query,
                    json!({"commands":[{"name":"review","source":"extension"}]}),
                ),
                &Mapper,
            )
            .unwrap();
        let catalog = runtime.available_commands().snapshot();
        assert_eq!(catalog.revision, turn + 1);
        catalog.validate(catalog.revision, "/review").unwrap();
        let mut prompt = Prompt::new("prompt", "/review").unwrap();
        prompt.command_revision = Some(catalog.revision);
        let send = runtime
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new("client", format!("review-{turn}")).unwrap(),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .expect("discovery must not exhaust prompt capacity");
        let send = deliver(&mut runtime, send);
        let batch = runtime
            .ingest(&response(&send, Value::Null), &Mapper)
            .unwrap();
        assert!(batch.events().iter().any(|event| matches!(
            event,
            AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Completed,
                ..
            }
        )));
    }
}

#[test]
fn pi_discovers_only_advertised_commands_and_replaces_the_catalog() {
    let mut runtime = PiRuntime::fixture(
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("slash-pi").unwrap(),
    );
    ready(&mut runtime);
    assert!(runtime.available_commands().snapshot().commands.is_empty());
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(&mut runtime, query);
    assert_eq!(query["type"], "get_commands");
    assert!(
        runtime.discover_commands().unwrap().is_none(),
        "bounded outstanding discovery"
    );
    runtime.ingest(&response(&query, json!({"commands":[{"name":"review","description":"Review changes","source":"extension"}]})), &Mapper).unwrap();
    let commands = runtime.available_commands().snapshot();
    assert_eq!(commands.commands[0].name, "review");
    assert_eq!(commands.commands[0].input_hint, None);
    commands.validate(commands.revision, "/review src").unwrap();
    assert!(
        runtime.discover_commands().unwrap().is_none(),
        "no idle polling loop"
    );
    let mut prompt = Prompt::new("prompt", "/review src").unwrap();
    prompt.command_revision = Some(commands.revision);
    let send = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "command-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let send = deliver(&mut runtime, send);
    assert_eq!(send["type"], "prompt");
    assert_eq!(send["message"], "/review src");
    let done = runtime
        .ingest(&response(&send, Value::Null), &Mapper)
        .unwrap();
    assert!(
        done.events().iter().any(|e| matches!(
            e,
            AdapterEvent::Turn {
                state: crate::acp::lifecycle::TurnState::Completed,
                ..
            }
        )),
        "extension without model work must complete on acknowledgment"
    );
    assert!(runtime.active_turn.is_none());
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(&mut runtime, query);
    runtime.ingest(&frame(json!({"type":"response","id":query["id"],"command":"get_commands","success":false,"error":"unsupported"})), &Mapper).unwrap();
    assert_eq!(
        names(&runtime.available_commands().snapshot().commands),
        ["compact"]
    );
}
#[test]
fn pi_rejects_uncorrelated_command_advertisements() {
    let mut runtime = PiRuntime::fixture(
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("slash-pi").unwrap(),
    );
    ready(&mut runtime);
    assert!(runtime.ingest(&frame(json!({"type":"response","id":"foreign","command":"get_commands","success":true,"data":{"commands":[{"name":"invented"}]}})), &Mapper).is_err());
    assert!(runtime.available_commands().snapshot().commands.is_empty());
}

#[test]
fn extension_model_events_before_ack_are_owned_and_late_ack_does_not_complete_twice() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(&mut runtime, query);
    runtime
        .ingest(
            &response(
                &query,
                json!({"commands":[{"name":"review","source":"extension"}]}),
            ),
            &Mapper,
        )
        .unwrap();
    let mut prompt = Prompt::new("prompt", "/review").unwrap();
    prompt.command_revision = Some(1);
    let send = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "ext").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let send = deliver(&mut runtime, send);
    runtime
        .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
        .unwrap();
    let ack = runtime
        .ingest(&response(&send, Value::Null), &Mapper)
        .unwrap();
    assert!(
        !ack.events()
            .iter()
            .any(|e| matches!(e, AdapterEvent::Turn { state, .. } if state.is_terminal()))
    );
    runtime
        .ingest(&frame(json!({"type":"agent_end","messages":[]})), &Mapper)
        .unwrap();
    let done = runtime
        .ingest(&frame(json!({"type":"agent_settled"})), &Mapper)
        .unwrap();
    assert!(
        done.events()
            .iter()
            .any(|e| matches!(e, AdapterEvent::Turn { state, .. } if state.is_terminal()))
    );
    let duplicate = runtime
        .ingest(&response(&send, Value::Null), &Mapper)
        .unwrap();
    assert!(
        !duplicate
            .events()
            .iter()
            .any(|e| matches!(e, AdapterEvent::Turn { state, .. } if state.is_terminal()))
    );
}

fn discovered(runtime: &mut PiRuntime, commands: Value) -> u64 {
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(runtime, query);
    runtime
        .ingest(&response(&query, json!({ "commands": commands })), &Mapper)
        .unwrap();
    runtime.available_commands().snapshot().revision
}

fn slash(runtime: &mut PiRuntime, text: &str, revision: u64) -> Value {
    let mut prompt = Prompt::new("prompt", text).unwrap();
    prompt.command_revision = Some(revision);
    let send = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "slash").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    deliver(runtime, send)
}

fn turn_states(batch: &AdapterBatch) -> Vec<crate::acp::lifecycle::TurnState> {
    batch
        .events()
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Turn { state, .. } => Some(*state),
            _ => None,
        })
        .collect()
}

#[test]
fn compact_is_listed_once_and_takes_the_place_of_a_same_named_template() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    discovered(
        &mut runtime,
        json!([{"name":"compact","source":"prompt","description":"Mine"},
            {"name":"fix-tests","source":"prompt","description":"Fix failing tests"}]),
    );
    let names: Vec<_> = runtime
        .available_commands()
        .snapshot()
        .commands
        .iter()
        .map(|command| (command.name.clone(), command.description.clone()))
        .collect();
    assert_eq!(
        names,
        [
            ("fix-tests".into(), "Fix failing tests".into()),
            (
                "compact".into(),
                "Summarize the conversation so far to free up context".into()
            ),
        ]
    );
}

#[test]
fn slash_compact_runs_pis_compaction_as_a_turn_with_its_compaction_item() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let revision = discovered(&mut runtime, json!([]));
    let request = slash(&mut runtime, "/compact  keep the API notes ", revision);
    assert_eq!(
        request,
        json!({"type":"compact","id":request["id"],"customInstructions":"keep the API notes"})
    );
    // Pi compacts before it answers, so its events arrive first.
    let started = runtime
        .ingest(
            &frame(json!({"type":"compaction_start","reason":"manual"})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(started.events(), [AdapterEvent::Item { .. }]));
    let ended = runtime
        .ingest(
            &frame(
                json!({"type":"compaction_end","reason":"manual","aborted":false,
                "willRetry":false,"result":{"summary":"s","tokensBefore":9000}}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(ended.events(), [AdapterEvent::Item { .. }]));
    let done = runtime
        .ingest(&response(&request, json!({"summary":"s"})), &Mapper)
        .unwrap();
    assert_eq!(
        turn_states(&done),
        [crate::acp::lifecycle::TurnState::Completed]
    );
    // A bare `/compact` sends no instructions, and the session takes prompts again.
    let request = slash(&mut runtime, "/compact", revision);
    assert_eq!(request, json!({"type":"compact","id":request["id"]}));
    // Pi reports a session too small to compact as a failed compaction; its
    // reason reaches the user as a notice beside the failed compaction item.
    runtime
        .ingest(
            &frame(json!({"type":"compaction_start","reason":"manual"})),
            &Mapper,
        )
        .unwrap();
    let ended = runtime
        .ingest(
            &frame(
                json!({"type":"compaction_end","reason":"manual","aborted":false,"willRetry":false,
                "errorMessage":"Compaction failed: Nothing to compact (session too small)"}),
            ),
            &Mapper,
        )
        .unwrap();
    let [
        AdapterEvent::Item { .. },
        AdapterEvent::Session { event, .. },
    ] = ended.events()
    else {
        panic!("failed compaction and its reason, got {:?}", ended.events())
    };
    let crate::acp::intent::session_event::SessionScopedEvent::Notice(notice) = event else {
        panic!("notice expected")
    };
    assert_eq!(
        notice.text.as_str(),
        "Compaction failed: Nothing to compact (session too small)"
    );
    let failed = runtime
        .ingest(
            &frame(
                json!({"type":"response","id":request["id"],"command":"compact",
                "success":false,"error":"Nothing to compact"}),
            ),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        turn_states(&failed),
        [crate::acp::lifecycle::TurnState::Failed]
    );
    running(&mut runtime);
}

#[test]
fn other_slash_text_still_goes_to_pi_as_a_prompt() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let revision = discovered(
        &mut runtime,
        json!([{"name":"fix-tests","source":"prompt","description":"Fix failing tests"}]),
    );
    let request = slash(&mut runtime, "/fix-tests now", revision);
    assert_eq!(
        (request["type"].as_str(), request["message"].as_str()),
        (Some("prompt"), Some("/fix-tests now"))
    );
}

fn names(commands: &[crate::acp::slash_commands::AvailableCommand]) -> Vec<&str> {
    commands
        .iter()
        .map(|command| command.name.as_str())
        .collect()
}
