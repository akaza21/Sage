use super::*;
use crate::acp::bindings::runtime::NativeRuntime;
fn update(session: &str, list: Value) -> Frame {
    frame(
        json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":session,"update":{"sessionUpdate":"available_commands_update","availableCommands":list}}}),
    )
}
fn command(revision: u64) -> AdapterCommand {
    let mut prompt = Prompt::new("prompt", "/plan task").unwrap();
    prompt.command_revision = Some(revision);
    AdapterCommand::Prompt {
        prompt,
        client_message_id: RequiredText::new("client", "slash-client").unwrap(),
        access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
    }
}
#[test]
fn legacy_constructor_keeps_advertised_version_gated_commands_dispatchable() {
    use crate::acp::bindings::claude::admission::ClaudeBindingRegistration;
    use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};
    let root = std::env::temp_dir().join(format!("sage-legacy-commands-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let adapter = root.join("adapter.js");
    let node = root.join("node.exe");
    std::fs::write(&adapter, b"adapter").unwrap();
    std::fs::write(&node, b"node").unwrap();
    let binding = ClaudeBindingRegistration::admitted_for_test(ClaudeLaunchDiscovery::Present(
        Box::new(DiscoveredClaudeLaunch {
            adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
            node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
            adapter_path: adapter,
            node_path: node,
            adapter_version: "0.76.0".into(),
            acp_sdk_version: "1.4.0".into(),
            node_engine: ">=22".into(),
            node_version: "22.18.0".into(),
        }),
    ))
    .refresh()
    .unwrap();
    let mut runtime = ClaudeRuntime::from_admitted_binding(
        binding,
        RuntimeEpoch::new(3).unwrap(),
        SageAgentSessionId::parse("session-claude-runtime").unwrap(),
    )
    .unwrap();
    ready(&mut runtime);
    runtime
        .ingest(
            &update(
                "claude-session-1",
                json!([
                    {"name":"reload-skills","description":"Reload skills"}
                ]),
            ),
            &Mapper,
        )
        .unwrap();
    let list = runtime.available_commands().snapshot();
    assert_eq!(list.commands.len(), 1);
    assert_eq!(
        serde_json::to_value(&list.commands[0]).unwrap()["placement"],
        json!({"section":"skillsAndCommands"})
    );
    let mut prompt = Prompt::new("prompt", "/reload-skills").unwrap();
    prompt.command_revision = Some(list.revision);
    let wire = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "legacy-command").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::Ask,
        })
        .unwrap();
    let wire: Value = serde_json::from_slice(wire.frame().payload()).unwrap();
    assert_eq!(
        wire["params"]["prompt"],
        json!([{"type":"text","text":"/reload-skills"}])
    );
    std::fs::remove_dir_all(root).unwrap();
}
#[test]
fn oversized_acp_catalog_reaches_shared_admission_through_wire_ingress() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let commands: Vec<_> = (0..600)
        .map(|i| json!({"name":format!("cmd{i:03}"),"description":"Command"}))
        .collect();
    let incoming = update("claude-session-1", json!(commands));
    crate::acp::transport::validate_structure(&incoming).unwrap();
    runtime.ingest(&incoming, &Mapper).unwrap();
    let list = runtime.available_commands().snapshot();
    assert_eq!((list.commands.len(), list.omitted), (512, 88));
    assert_eq!(list.commands[0].name, "cmd000");
    assert_eq!(list.commands.last().unwrap().name, "cmd511");
    assert!(list.validate(list.revision, "/cmd511").is_ok());
    assert!(list.validate(list.revision, "/cmd512").is_err());
}
#[test]
fn native_command_validates_latest_list_and_uses_acp_prompt_wire() {
    let mut claude = ClaudeRuntime::fixture();
    ready(&mut claude);
    let mut runtime = NativeRuntime::new(claude);
    assert!(runtime.available_commands().snapshot().commands.is_empty());
    assert!(runtime.command(command(0)).is_err());
    runtime
        .ingest(
            &update(
                "claude-session-1",
                json!([{"name":"plan","description":"Plan","input":{"hint":"task"}},
                    {"name":"codex:review","description":"(codex) Review"}]),
            ),
            &Mapper,
        )
        .unwrap();
    let listed = runtime.available_commands().snapshot();
    assert_eq!(
        listed.commands[1].source,
        crate::acp::slash_commands::CommandSource::Plugin
    );
    assert_eq!(listed.commands[1].source_label.as_deref(), Some("codex"));
    assert!(runtime.command(command(0)).is_err());
    let egress = runtime.command(command(1)).unwrap();
    let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(wire["method"], "session/prompt");
    assert_eq!(wire["params"]["sessionId"], "claude-session-1");
    assert_eq!(
        wire["params"]["prompt"],
        json!([{"type":"text","text":"/plan task"}])
    );
    assert!(runtime.command(command(1)).is_err(), "active turn conflict");
}
#[test]
fn foreign_updates_cannot_replace_the_list_and_malformed_updates_revoke_it() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    runtime
        .ingest(
            &update(
                "claude-session-1",
                json!([{"name":"plan","description":"Plan"}]),
            ),
            &Mapper,
        )
        .unwrap();
    assert!(
        runtime
            .ingest(&update("foreign", json!([])), &Mapper)
            .is_err()
    );
    assert_eq!(
        runtime.available_commands().snapshot().commands[0].name,
        "plan"
    );
    assert!(
        runtime
            .ingest(
                &update("claude-session-1", json!([{"name":"plan"}])),
                &Mapper
            )
            .is_err()
    );
    assert!(runtime.available_commands().snapshot().commands.is_empty());
}
#[test]
fn runtime_drop_clears_published_catalog_and_old_epochs_do_not_clear_new() {
    use crate::acp::slash_commands::CommandChannel;
    use sage_acp_protocol::scheduler::RuntimeOwner;
    let mut claude = ClaudeRuntime::fixture();
    ready(&mut claude);
    let owner = RuntimeOwner {
        project_id: sage_acp_protocol::contract::SageProjectId::parse("p").unwrap(),
        chat_id: sage_acp_protocol::contract::SageChatId::parse("c").unwrap(),
        session_id: claude.session_id.clone(),
        runtime_epoch: claude.runtime_epoch,
    };
    let channel = Arc::new(CommandChannel::default());
    channel.reserve(owner.clone());
    let mut runtime = NativeRuntime::new(claude);
    runtime.attach_commands(owner.clone(), channel.clone());
    runtime
        .receive_command_update(&update(
            "claude-session-1",
            json!([{"name":"plan","description":"Plan"}]),
        ))
        .unwrap();
    assert_eq!(
        channel.snapshot(&owner).unwrap().snapshot().commands.len(),
        1
    );
    drop(runtime);
    assert!(channel.snapshot(&owner).is_none());
}
