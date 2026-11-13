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
    list.replace(parse_acp(
        &json!([{"name":"plan","description":"Plan work","input":{"hint":"task"}}]),
    ))
    .unwrap();
    assert_eq!(list.commands[0].input_hint.as_deref(), Some("task"));
    assert!(list.validate(1, "/plan something").is_ok());
    list.replace(parse_acp(&json!([{"name":"test","description":"Test"}])))
        .unwrap();
    assert!(list.validate(1, "/plan something").is_err());
    assert!(list.validate(1, "/test").is_err());
    assert!(list.validate(2, "/test").is_ok());
    list.replace(parse_acp(&json!([]))).unwrap();
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
        json!([{"name":"x","description":"d","input":{"hint":"h".repeat(257)}}]),
        json!(vec![
            json!({"name":"x","description":"d"});
            MAX_COMMANDS + 1
        ]),
        json!(null),
    ] {
        let mut list = CommandList::default();
        list.replace(parse_acp(&json!([{"name":"old","description":"old"}])))
            .unwrap();
        assert!(list.replace(parse_acp(&value)).is_err());
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
    list.replace(parse_acp(&json!([{"name":"plan","description":"d"}])))
        .unwrap();
    let state = CommandState::default();
    state.replace(Ok(list.commands.clone())).unwrap();
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
    let values: Vec<_> = (0..MAX_COMMANDS).map(|i| json!({"name":format!("cmd{i:03}"),"description":"d".repeat(400),"input":{"hint":"hint"}})).collect();
    let mut list = CommandList::default();
    list.replace(parse_acp(&json!(values))).unwrap();
    let settled = retained(&list);
    let mut replacement = list.clone();
    replacement.replace(parse_acp(&json!(values))).unwrap();
    let peak = retained(&list) + retained(&replacement);
    assert!(
        settled
            <= MAX_COMMAND_BYTES
                + MAX_COMMANDS * std::mem::size_of::<AvailableCommand>()
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
        .replace(parse_acp(&json!([{"name":"plan","description":"d"}])))
        .unwrap();
    channel.update(&owner, driver.clone());
    channel.clear(&owner);
    assert!(driver.snapshot().commands.is_empty());
    assert!(driver.validate(1, "/plan").is_err());
    assert!(
        driver
            .replace(parse_acp(&json!([{"name":"late","description":"d"}])))
            .is_err()
    );
}

#[test]
fn channel_announces_each_new_revision_once_for_the_exact_owner() {
    let channel = CommandChannel::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink_seen = Arc::clone(&seen);
    channel.set_sink(Arc::new(move |notice: CommandsChangedV1| {
        sink_seen.lock().unwrap().push(notice);
    }));
    let live = owner(1);
    channel.reserve(live.clone());
    let state = Arc::new(CommandState::default());
    // Adopting the empty list at revision 0 is not a change.
    channel.update(&live, Arc::clone(&state));
    assert!(seen.lock().unwrap().is_empty());
    state
        .replace(parse_acp(&json!([{"name":"plan","description":"d"}])))
        .unwrap();
    channel.update(&live, Arc::clone(&state));
    // Every ingested frame calls update; one revision is announced once.
    channel.update(&live, Arc::clone(&state));
    channel.update(&owner(2), Arc::clone(&state));
    state.replace(parse_acp(&json!([]))).unwrap();
    channel.update(&live, Arc::clone(&state));
    let seen = seen.lock().unwrap();
    assert_eq!(
        seen.iter().map(|n| n.revision).collect::<Vec<_>>(),
        vec![1, 2]
    );
    let wire = serde_json::to_value(&seen[0]).unwrap();
    assert_eq!(wire["schemaVersion"], 1);
    assert_eq!(wire["sageChatId"], "chat");
    assert_eq!(wire["sageAgentSessionId"], "session");
    assert_eq!(wire["runtimeEpoch"], "1");
    assert_eq!(wire["revision"], 1);
}

#[test]
fn channel_announces_a_swapped_list_from_its_own_revisions() {
    let channel = CommandChannel::default();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink_seen = Arc::clone(&seen);
    channel.set_sink(Arc::new(move |notice: CommandsChangedV1| {
        sink_seen.lock().unwrap().push(notice.revision);
    }));
    let live = owner(1);
    channel.reserve(live.clone());
    let first = Arc::new(CommandState::default());
    first
        .replace(parse_acp(&json!([{"name":"plan","description":"d"}])))
        .unwrap();
    channel.update(&live, Arc::clone(&first));
    // A fresh empty state at revision 0 replaces it: nothing to announce.
    let second = Arc::new(CommandState::default());
    channel.update(&live, Arc::clone(&second));
    // Its first list is revision 1 again and must still be announced.
    second
        .replace(parse_acp(&json!([{"name":"init","description":"d"}])))
        .unwrap();
    channel.update(&live, Arc::clone(&second));
    assert_eq!(*seen.lock().unwrap(), vec![1, 1]);
    assert!(first.snapshot().commands.is_empty());
    assert_eq!(
        channel.snapshot(&live).unwrap().snapshot().commands[0].name,
        "init"
    );
}

#[test]
fn acp_commands_carry_the_source_an_adapter_names_and_list_as_prompts() {
    let mut list = CommandList::default();
    list.replace(parse_acp(&json!([
        {"name":"review","description":"Review","_meta":{"source":"skill"}},
        {"name":"mcp__docs__search","description":"Search","_meta":{"source":"mcp"}},
        {"name":"init","description":"Init","_meta":{"source":"invented"}},
        {"name":"plan","description":"Plan","_meta":null},
    ])))
    .unwrap();
    let sources: Vec<_> = list.commands.iter().map(|c| c.source).collect();
    assert_eq!(
        sources,
        [
            CommandSource::Skill,
            CommandSource::Mcp,
            CommandSource::Harness,
            CommandSource::Harness
        ]
    );
    assert!(
        list.commands
            .iter()
            .all(|c| c.dispatch == CommandDispatch::Prompt)
    );
    let wire = serde_json::to_value(&list.commands[0]).unwrap();
    assert_eq!(
        wire,
        json!({"name":"review","description":"Review","inputHint":null,"source":"skill"})
    );
    // A run names the source of the command it ran, at that revision only.
    let state = CommandState::default();
    state.replace(Ok(list.commands)).unwrap();
    assert_eq!(
        state.command(1, "/review the diff").map(|c| c.source),
        Ok(CommandSource::Skill)
    );
    assert_eq!(
        state.command(1, "/plan").map(|c| c.source),
        Ok(CommandSource::Harness)
    );
    assert!(state.command(0, "/review").map(|c| c.source).is_err());
    assert!(state.command(1, "/missing").map(|c| c.source).is_err());
}

#[test]
fn a_long_description_is_clipped_and_the_list_still_admitted() {
    let mut list = CommandList::default();
    let long = format!("{}é", "d".repeat(1023));
    list.replace(parse_acp(&json!([{"name":"skill","description":long}])))
        .unwrap();
    assert_eq!(list.commands[0].description, "d".repeat(1023));
}

#[test]
fn aliases_from_acp_meta_keep_only_distinct_command_names_within_the_bound() {
    let mut aliases = vec![
        json!("cost"),
        json!("bad name"),
        json!("usage"),
        json!("cost"),
        json!(3),
    ];
    aliases.extend((0..10).map(|n| json!(format!("a{n}"))));
    let [command] =
        &parse_acp(&json!([{"name":"usage","description":"Usage","_meta":{"aliases":aliases}}]))
            .unwrap()[..]
    else {
        panic!("one command");
    };
    assert_eq!(
        command.aliases,
        ["cost", "a0", "a1", "a2", "a3", "a4", "a5", "a6"]
    );
    assert_eq!(serde_json::to_value(command).unwrap()["aliases"][0], "cost");
    let plain = &parse_acp(&json!([{"name":"x","description":"d"}])).unwrap()[0];
    assert!(
        serde_json::to_value(plain)
            .unwrap()
            .get("aliases")
            .is_none()
    );
}
