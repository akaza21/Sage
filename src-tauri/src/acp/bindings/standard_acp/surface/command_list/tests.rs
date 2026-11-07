use super::*;
use serde_json::json;
fn owner(epoch: u64) -> RuntimeOwner {
    use crate::acp::contract::*;
    RuntimeOwner {
        project_id: SageProjectId::parse("project").unwrap(),
        chat_id: SageChatId::parse("chat").unwrap(),
        session_id: SageAgentSessionId::parse("session").unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}
#[test]
fn latest_list_replaces_and_revokes_prior_command_authority() {
    let mut list = CommandList::default();
    assert!(list.commands.is_empty());
    assert!(list.validate(0, "/invented").is_err());
    list.replace(
        &json!([{"name":"plan","description":"Plan work","input":{"hint":"task"}}]),
        false,
    )
    .unwrap();
    assert_eq!(list.commands[0].input_hint.as_deref(), Some("task"));
    assert!(list.validate(1, "/plan something").is_ok());
    list.replace(&json!([{"name":"test","description":"Test"}]), false)
        .unwrap();
    assert!(list.validate(1, "/plan something").is_err());
    assert!(list.validate(1, "/test").is_err());
    assert!(list.validate(2, "/test").is_ok());
    list.replace(&json!([]), false).unwrap();
    assert!(list.commands.is_empty());
    assert!(list.validate(3, "/test").is_err());
}
#[test]
fn malformed_duplicate_and_over_cap_lists_fail_closed() {
    for value in [
        json!([{"name":"bad name","description":"d"}]),
        json!([{"name":"x","description":"d"},{"name":"x","description":"d"}]),
        json!([{"name":"/x","description":"d"}]),
        json!([{"name":"x","description":"d","input":{}}]),
        json!([{"name":"x","description":"x".repeat(1025)}]),
        json!(vec![json!({"name":"x","description":"d"}); 129]),
        json!(null),
    ] {
        let mut list = CommandList::default();
        list.replace(&json!([{"name":"old","description":"old"}]), false)
            .unwrap();
        assert!(list.replace(&value, false).is_err());
        assert!(list.commands.is_empty());
        assert!(list.validate(1, "/old").is_err());
    }
}
#[test]
fn channel_retains_only_latest_exact_owner_and_clears_after_release() {
    let channel = CommandChannel::default();
    let old = owner(1);
    let next = owner(2);
    channel.reserve(old.clone());
    let mut list = CommandList::default();
    list.replace(&json!([{"name":"plan","description":"d"}]), false)
        .unwrap();
    let state = CommandState::default();
    state
        .replace(&serde_json::to_value(&list.commands).unwrap(), true)
        .unwrap();
    let list = Arc::new(state);
    let weak = Arc::downgrade(&list);
    channel.update(&old, list);
    assert_eq!(channel.snapshot(&old).unwrap().snapshot().commands.len(), 1);
    channel.reserve(next.clone());
    assert!(weak.upgrade().is_none());
    channel.update(&old, Arc::default());
    channel.clear(&old);
    assert!(channel.snapshot(&old).is_none());
    assert!(
        channel
            .snapshot(&next)
            .unwrap()
            .snapshot()
            .commands
            .is_empty()
    );
    channel.clear(&next);
    assert!(channel.snapshot(&next).is_none());
}
#[test]
fn command_marker_is_native_only_and_survives_host_prompt_clones() {
    let mut prompt = crate::acp::intent::Prompt::new("prompt", "/plan task").unwrap();
    prompt.command_revision = Some(7);
    assert_eq!(prompt.clone().command_revision, Some(7));
    let wire = serde_json::to_value(&prompt).unwrap();
    assert_eq!(wire, json!("/plan task"));
    let decoded: crate::acp::intent::Prompt = serde_json::from_value(wire).unwrap();
    assert_eq!(decoded.command_revision, None);
}
#[test]
fn command_catalog_memory_peak_settled_and_release() {
    fn retained(list: &CommandList) -> usize {
        std::mem::size_of::<CommandList>()
            + list.commands.capacity() * std::mem::size_of::<AvailableCommand>()
            + list
                .commands
                .iter()
                .map(|c| {
                    c.name.capacity()
                        + c.description.capacity()
                        + c.input_hint.as_ref().map_or(0, String::capacity)
                })
                .sum::<usize>()
    }
    let values: Vec<_> = (0..128).map(|i| json!({"name":format!("cmd{i:03}"),"description":"d".repeat(1000),"input":{"hint":"hint"}})).collect();
    let mut list = CommandList::default();
    list.replace(&json!(values), false).unwrap();
    let settled = retained(&list);
    let mut replacement = list.clone();
    replacement.replace(&json!(values), false).unwrap();
    let peak = retained(&list) + retained(&replacement);
    assert!(
        settled
            <= MAX_COMMAND_BYTES
                + 128 * std::mem::size_of::<AvailableCommand>()
                + std::mem::size_of::<CommandList>()
    );
    drop(list);
    drop(replacement);
    println!(
        "command catalog owned allocation bytes: replacement peak={peak}, settled={settled}, after release=0; excludes input wire buffers, allocator overhead and process RSS"
    );
}

#[test]
fn teardown_revokes_and_frees_catalog_even_when_driver_is_retained() {
    let channel = CommandChannel::default();
    let owner = owner(1);
    channel.reserve(owner.clone());
    let driver = Arc::new(CommandState::default());
    driver
        .replace(&json!([{"name":"plan","description":"d"}]), false)
        .unwrap();
    channel.update(&owner, driver.clone());
    channel.clear(&owner);
    assert!(driver.snapshot().commands.is_empty());
    assert!(driver.validate(1, "/plan").is_err());
    assert!(
        driver
            .replace(&json!([{"name":"late","description":"d"}]), false)
            .is_err()
    );
}
