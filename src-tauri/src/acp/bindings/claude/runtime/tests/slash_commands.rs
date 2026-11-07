use super::*;
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
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    }
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
                json!([{"name":"plan","description":"Plan","input":{"hint":"task"}}]),
            ),
            &Mapper,
        )
        .unwrap();
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
    use crate::acp::{scheduler::RuntimeOwner, slash_commands::CommandChannel};
    let mut claude = ClaudeRuntime::fixture();
    ready(&mut claude);
    let owner = RuntimeOwner {
        project_id: crate::acp::contract::SageProjectId::parse("p").unwrap(),
        chat_id: crate::acp::contract::SageChatId::parse("c").unwrap(),
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
