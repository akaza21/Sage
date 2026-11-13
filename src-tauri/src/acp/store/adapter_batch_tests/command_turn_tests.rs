use super::*;
use crate::acp::slash_commands::{CommandMark, CommandSource};
use serde_json::json;

/// Ends the fixture's Turn and prepares the next with this prompt.
fn prepare(fixture: &mut Fixture, prompt: Prompt, intent: &str) {
    let owner = fixture.owner.clone();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &owner,
            [TurnState::Running, TurnState::Completed]
                .map(|state| AdapterInboundEvent::Turn {
                    state,
                    now_millis: 7,
                })
                .into(),
        ),
    )
    .unwrap();
    let expected_projection_revision = projection(&fixture.connection);
    prepare_turn(
        &mut fixture.connection,
        PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision,
            client_intent_id: ClientIntentId::parse(intent).unwrap(),
            prompt,
            now_millis: 9,
        },
    )
    .unwrap();
}

#[test]
fn a_command_turn_reloads_as_the_command_it_ran() {
    let mut fixture = fixture();
    let mut command = Prompt::new("command", "/deploy staging").unwrap();
    command.command = Some(CommandMark {
        name: "deploy".into(),
        source: CommandSource::Skill,
    });
    let stored = json!({"kind":"text","payload":command.user_payload()}).to_string();
    assert!(crate::acp::store::decode_payload(&stored).is_ok());
    prepare(&mut fixture, command, "command-turn");
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id.clone(),
        fixture.owner.chat_id.clone(),
        None,
    )
    .unwrap();
    let mut turns = page
        .turns
        .as_slice()
        .iter()
        .map(|turn| serde_json::to_value(turn).unwrap())
        .collect::<Vec<_>>();
    turns.sort_by_key(|turn| turn["ordinal"].as_u64());
    let [plain, command] = turns.as_slice() else {
        panic!("two turns")
    };
    assert_eq!(plain.get("command"), None);
    assert_eq!(
        command["command"],
        json!({"name":"deploy","source":"skill"})
    );
    assert_eq!(command["items"][0]["payload"]["text"], "/deploy staging");
}
