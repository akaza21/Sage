use super::*;

fn value(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).unwrap()
}

#[test]
fn shared_refresh_intents_coalesce_until_the_harness_answers() {
    let mut runtime = NativeRuntime::new(ready());
    let commands = runtime.available_commands();
    let revision = commands.snapshot().revision;
    let channel = Arc::new(crate::acp::slash_commands::CommandChannel::default());
    let owner = sage_acp_protocol::scheduler::RuntimeOwner {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-refresh").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-refresh").unwrap(),
        session_id: SageAgentSessionId::parse("session-claude-sdk").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    channel.reserve(owner.clone());
    let published = Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = published.clone();
    let catalog = commands.clone();
    channel.set_sink(Arc::new(move |_| {
        let list = catalog.snapshot();
        received.lock().unwrap().push(list.revision);
    }));
    channel.update(&owner, commands.clone());
    published.lock().unwrap().clear();
    runtime.attach_commands(owner, channel);
    commands.request_refresh();
    let query = runtime.discover_commands().unwrap().unwrap();
    let request = value(&query);
    assert_eq!(request["request"]["subtype"], "initialize");
    assert_eq!(commands.snapshot().revision, revision);
    assert!(published.lock().unwrap().is_empty());
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"type":"system","subtype":"commands_changed","commands":[{"name":"pushed","builtin":true}]})), &Mapper).unwrap();
    commands.request_refresh();
    commands.request_refresh();
    assert!(runtime.discover_commands().unwrap().is_none());
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":{"commands":[{"name":"fresh","builtin":true}]}}})), &Mapper).unwrap();
    let list = commands.snapshot();
    assert!(list.commands.iter().any(|command| command.name == "fresh"));
    assert_eq!(published.lock().unwrap().last(), Some(&list.revision));
    assert!(runtime.discover_commands().unwrap().is_none());
}

#[test]
fn a_refused_catalog_read_revokes_the_list_and_allows_another_refresh() {
    let mut runtime = NativeRuntime::new(ready());
    let commands = runtime.available_commands();
    let selected = commands.snapshot().revision;
    commands.request_refresh();
    let query = runtime.discover_commands().unwrap().unwrap();
    let request = value(&query);
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"error","request_id":request["request_id"],"error":"catalog unavailable"}})),&Mapper).unwrap();
    let mut prompt = Prompt::new("command", "/review src").unwrap();
    prompt.command_revision = Some(selected);
    assert!(matches!(
        runtime.command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "revoked-review").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        }),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    commands.request_refresh();
    let next = runtime.discover_commands().unwrap().unwrap();
    assert_eq!(value(&next)["request"]["subtype"], "initialize");
    assert_ne!(value(&next)["request_id"], request["request_id"]);
}

#[test]
fn completed_reload_placement_requests_a_new_claude_catalog() {
    let mut runtime = NativeRuntime::new(ready());
    runtime.ingest(&frame(json!({"type":"system","subtype":"commands_changed","commands":[{"name":"reload-skills","builtin":true}]})), &Mapper).unwrap();
    let list = runtime.available_commands().snapshot();
    let mut prompt = Prompt::new("command", "/reload-skills").unwrap();
    prompt.command_revision = Some(list.revision);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "reload-turn").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        })
        .unwrap();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    assert!(runtime.discover_commands().unwrap().is_none());
    let batch = runtime.ingest(&frame(finished()), &Mapper).unwrap();
    assert!(batch.events().iter().any(|event| matches!(
        event,
        AdapterEvent::Turn {
            state: TurnState::Completed,
            ..
        }
    )));
    let query = runtime.discover_commands().unwrap().unwrap();
    assert_eq!(value(&query)["request"]["subtype"], "initialize");
}

#[test]
fn identical_claude_refresh_preserves_a_selected_command_for_send() {
    let mut runtime = ready();
    let catalog =
        json!({"commands":[{"name":"review","description":"Review changes","source":"skill"}]});
    let query = runtime.refresh_commands().unwrap().unwrap();
    let request = value(&query);
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    reply(&mut runtime, &request, catalog.clone());
    let selected = runtime.available_commands().snapshot().revision;
    let mut prompt = Prompt::new("command", "/review src").unwrap();
    prompt.command_revision = Some(selected);
    let query = runtime.refresh_commands().unwrap().unwrap();
    let request = value(&query);
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    reply(&mut runtime, &request, catalog);
    let sent = send(
        &mut runtime,
        AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "selected-review").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        },
    );
    assert_eq!(sent["message"]["content"][0]["text"], "/review src");
    assert_eq!(runtime.available_commands().snapshot().revision, selected);
}

#[test]
fn a_new_session_sends_exactly_one_catalog_read() {
    let mut runtime = NativeRuntime::new(super::runtime());
    let mut reads = 0;
    let mut written = |runtime: &mut NativeRuntime, egress: AdapterEgress| {
        let sdk = value(&egress);
        if sdk["request"]["subtype"] == "initialize" {
            reads += 1;
        }
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Written)
            .unwrap();
        sdk
    };
    let initialize = runtime.command(AdapterCommand::Initialize).unwrap();
    let initialize = written(&mut runtime, initialize);
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":initialize["request_id"],"response":{"commands":[{"name":"review","description":"Review changes"}],"current_permission_mode":"default","models":[{"value":"default","displayName":"Default"}]}}})), &Mapper).unwrap();
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: "/w".into(),
            model: None,
        })
        .unwrap();
    let session = written(&mut runtime, session);
    let batch = runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":session["request_id"],"response":{"applied":{}}}})), &Mapper).unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::SessionReady { loaded: false, .. }, ..]
    ));
    assert!(runtime.available_commands().advertised());
    while let Some(egress) = runtime.discover_commands().unwrap() {
        written(&mut runtime, egress);
    }
    assert_eq!(reads, 1);
}
