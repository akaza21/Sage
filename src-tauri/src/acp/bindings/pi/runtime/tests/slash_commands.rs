use super::*;
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
    assert!(runtime.available_commands().snapshot().commands.is_empty());
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
