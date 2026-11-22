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
        received.lock().unwrap().push((list.revision, list.pending));
    }));
    runtime.attach_commands(owner, channel);
    commands.request_refresh();
    let query = runtime.discover_commands().unwrap().unwrap();
    let request = value(&query);
    assert_eq!(request["request"]["subtype"], "initialize");
    assert_eq!(commands.snapshot().revision, revision);
    assert_eq!(published.lock().unwrap().last(), Some(&(revision, true)));
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"type":"system","subtype":"commands_changed","commands":[{"name":"pushed","builtin":true}]})), &Mapper).unwrap();
    assert!(commands.snapshot().pending);
    commands.request_refresh();
    commands.request_refresh();
    assert!(runtime.discover_commands().unwrap().is_none());
    assert!(commands.snapshot().pending);
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":{"commands":[{"name":"fresh","builtin":true}]}}})), &Mapper).unwrap();
    let list = commands.snapshot();
    assert!(!list.pending);
    assert!(list.commands.iter().any(|command| command.name == "fresh"));
    assert_eq!(
        published.lock().unwrap().last(),
        Some(&(list.revision, false))
    );
    assert!(runtime.discover_commands().unwrap().is_none());
}

#[test]
fn a_refused_catalog_read_clears_pending_and_allows_another_refresh() {
    let mut runtime = NativeRuntime::new(ready());
    let commands = runtime.available_commands();
    commands.request_refresh();
    let query = runtime.discover_commands().unwrap().unwrap();
    let request = value(&query);
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"error","request_id":request["request_id"],"error":"catalog unavailable"}})),&Mapper).unwrap();
    assert!(!commands.snapshot().pending);
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
    assert!(runtime.available_commands().snapshot().pending);
}

#[test]
fn commands_changed_adds_and_removes_catalog_capabilities() {
    use crate::acp::slash_commands::CommandChannel;
    use sage_acp_protocol::{
        contract::{SageChatId, SageProjectId},
        scheduler::RuntimeOwner,
    };
    let owner = RuntimeOwner {
        project_id: SageProjectId::parse("project-catalog").unwrap(),
        chat_id: SageChatId::parse("chat-catalog").unwrap(),
        session_id: SageAgentSessionId::parse("session-claude-sdk").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let channel = Arc::new(CommandChannel::default());
    channel.reserve(owner.clone());
    let published = Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = published.clone();
    channel.set_recorder(Arc::new(move |_, capabilities| {
        received.lock().unwrap().push(capabilities.goal);
        true
    }));
    let mut runtime = NativeRuntime::new(ready());
    runtime.attach_commands(owner, channel);
    for (rows, supported) in [
        (json!([{"name":"goal","builtin":true}]), true),
        (json!([]), false),
        (json!([{"name":"goal","source":"skill"}]), false),
    ] {
        runtime
            .ingest(
                &frame(json!({"type":"system","subtype":"commands_changed","commands":rows})),
                &Mapper,
            )
            .unwrap();
        assert_eq!(runtime.capabilities().unwrap().goal.supported(), supported);
        assert_eq!(
            published.lock().unwrap().last().unwrap().supported(),
            supported
        );
        assert_eq!(
            runtime.goal_state() == Some(&crate::acp::goal::GoalState::Unsupported),
            !supported
        );
    }
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
fn missing_claude_catalog_reply_expires_during_other_traffic_and_late_reply_is_discarded() {
    let mut runtime = NativeRuntime::new(ready());
    let commands = runtime.available_commands();
    let channel = Arc::new(crate::acp::slash_commands::CommandChannel::default());
    let owner = sage_acp_protocol::scheduler::RuntimeOwner {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("project-expiry").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("chat-expiry").unwrap(),
        session_id: SageAgentSessionId::parse("session-claude-sdk").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    channel.reserve(owner.clone());
    let published = Arc::new(std::sync::Mutex::new(Vec::new()));
    let received = published.clone();
    let catalog = commands.clone();
    channel.set_sink(Arc::new(move |_| {
        received.lock().unwrap().push(catalog.snapshot().pending)
    }));
    runtime.attach_commands(owner, channel);
    commands.request_refresh();
    let query = runtime.discover_commands().unwrap().unwrap();
    let request = value(&query);
    runtime
        .resolve_egress(query.into_token(), EgressResolution::Written)
        .unwrap();
    let deadline = runtime
        .poll_command_refresh(std::time::Instant::now())
        .unwrap();
    // Session notices keep arriving, but they cannot extend the correlated request's deadline.
    runtime.ingest(&frame(json!({"type":"system","subtype":"commands_changed","commands":[{"name":"live","source":"skill"}]})), &Mapper).unwrap();
    for status in [Some("compacting"), None, Some("compacting"), None] {
        let batch = runtime
            .ingest(
                &frame(json!({"type":"system","subtype":"status","status":status})),
                &Mapper,
            )
            .unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::Session { .. }]));
        assert_eq!(
            runtime.poll_command_refresh(std::time::Instant::now()),
            Some(deadline)
        );
    }
    assert_eq!(
        runtime.poll_command_refresh(deadline - std::time::Duration::from_millis(1)),
        Some(deadline)
    );
    assert!(commands.snapshot().pending);
    assert!(runtime.poll_command_refresh(deadline).is_none());
    assert_eq!(published.lock().unwrap().last(), Some(&false));
    commands.request_refresh();
    let retry = runtime.discover_commands().unwrap().unwrap();
    let retry_request = value(&retry);
    runtime
        .resolve_egress(retry.into_token(), EgressResolution::Written)
        .unwrap();
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":request["request_id"],"response":{"commands":[{"name":"stale"}]}}})), &Mapper).unwrap();
    assert!(commands.snapshot().pending);
    assert!(
        commands
            .snapshot()
            .commands
            .iter()
            .any(|c| c.name == "live")
    );
    runtime.ingest(&frame(json!({"type":"control_response","response":{"subtype":"success","request_id":retry_request["request_id"],"response":{"commands":[{"name":"fresh"}]}}})), &Mapper).unwrap();
    assert!(!commands.snapshot().pending);
    assert!(
        commands
            .snapshot()
            .commands
            .iter()
            .any(|c| c.name == "fresh")
    );
}
