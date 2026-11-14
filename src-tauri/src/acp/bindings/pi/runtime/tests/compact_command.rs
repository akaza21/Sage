use super::*;

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
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    deliver(runtime, send)
}

fn turn_states(batch: &AdapterBatch) -> Vec<sage_acp_protocol::lifecycle::TurnState> {
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
            ("session".into(), "Show session info and stats".into()),
            (
                "compact".into(),
                "Manually compact the session context".into()
            ),
        ]
    );
    assert_eq!(
        runtime.available_commands().snapshot().commands[1].dispatch,
        crate::acp::slash_commands::CommandDispatch::Panel(
            crate::acp::slash_commands::CommandPanel::Status
        )
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
        [sage_acp_protocol::lifecycle::TurnState::Completed]
    );
    let request = slash(&mut runtime, "/compact", revision);
    assert_eq!(request, json!({"type":"compact","id":request["id"]}));
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
    let sage_acp_protocol::intent::session_event::SessionScopedEvent::Notice(notice) = event else {
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
        [sage_acp_protocol::lifecycle::TurnState::Failed]
    );
    assert!(
        !failed
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::Error(_))),
        "the notice already gave the reason"
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

#[test]
fn a_full_catalog_keeps_its_size_and_still_lists_compact() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let max = crate::acp::bindings::standard_acp::surface::command_list::MAX_COMMANDS;
    let templates: Vec<_> = (0..max)
        .map(|n| json!({"name":format!("t{n}"),"source":"prompt","description":"T"}))
        .collect();
    discovered(&mut runtime, Value::Array(templates));
    let listed = runtime.available_commands().snapshot().commands;
    assert_eq!(listed.len(), max);
    assert_eq!(listed[max - 3].name, format!("t{}", max - 3));
    assert_eq!(listed[max - 2].name, "session");
    assert_eq!(listed[max - 1].name, "compact");
}

#[test]
fn a_compact_pi_rejects_outright_still_reports_the_failure() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let revision = discovered(&mut runtime, json!([]));
    let request = slash(&mut runtime, "/compact", revision);
    let failed = runtime
        .ingest(
            &frame(
                json!({"type":"response","id":request["id"],"command":"compact",
                "success":false,"error":"No model selected"}),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(
        failed
            .events()
            .iter()
            .any(|event| matches!(event, AdapterEvent::Error(_)))
    );
    assert_eq!(
        turn_states(&failed),
        [sage_acp_protocol::lifecycle::TurnState::Failed]
    );
}

#[test]
fn the_compact_action_runs_pis_compaction_without_a_listed_command() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    assert!(runtime.capabilities().unwrap().native_session_compact);
    let action = crate::acp::session_action::SessionAction::Compact {};
    let mut prompt = Prompt::new("prompt", action.prompt_text()).unwrap();
    prompt.action = Some(action);
    let send = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "compact-action").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let request = deliver(&mut runtime, send);
    assert_eq!(request, json!({"type":"compact","id":request["id"]}));
    let done = runtime
        .ingest(&response(&request, json!({"summary":"s"})), &Mapper)
        .unwrap();
    assert_eq!(
        turn_states(&done),
        [sage_acp_protocol::lifecycle::TurnState::Completed]
    );
}

#[test]
fn a_review_action_is_refused_since_pi_has_no_review() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    assert!(!runtime.capabilities().unwrap().native_review);
    let action = crate::acp::session_action::SessionAction::Review {
        target: crate::acp::session_action::ReviewTarget::UncommittedChanges {},
    };
    let mut prompt = Prompt::new("prompt", action.prompt_text()).unwrap();
    prompt.action = Some(action);
    assert!(matches!(
        runtime.command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "review-action").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    running(&mut runtime);
}
