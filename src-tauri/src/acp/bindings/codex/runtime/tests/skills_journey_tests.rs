use super::*;
use crate::acp::slash_commands::CommandSource;

const LONG: &str = "review-release-notes-for-the-synthetic-example-project-checklist";

fn wire(egress: &AdapterEgress) -> Value {
    serde_json::from_slice(egress.frame().payload()).unwrap()
}

fn journey(opened: &[Value], first_turn: Value, changed: &[Value]) -> Vec<Value> {
    let mut requests = vec![
        json!({"id":101,"method":"initialize"}),
        json!({"id":102,"method":"thread/start"}),
    ];
    requests.extend(opened.iter().cloned());
    requests.extend([json!({"id":103,"method":"thread/goal/get"}), first_turn]);
    requests.extend(changed.iter().cloned());
    requests.extend([
        json!({"id":105,"method":"turn/start","params":{"input":[]}}),
        json!({"id":106,"method":"thread/unsubscribe"}),
    ]);
    let requests: String = requests
        .iter()
        .map(|request| format!("{request}\n"))
        .collect();
    crate::acp::fixture::replay_native("codex-skills", &requests)
}

fn plain_turn() -> Value {
    json!({"id":104,"method":"turn/start","params":{"input":[]}})
}

fn read(runtime: &mut CodexRuntime, opened: Option<&Value>) -> Value {
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    let request = wire(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    let reply = match opened {
        None => journey(std::slice::from_ref(&request), plain_turn(), &[]),
        Some(opened) => journey(
            std::slice::from_ref(opened),
            plain_turn(),
            std::slice::from_ref(&request),
        ),
    }
    .into_iter()
    .find(|frame| frame["id"] == request["id"])
    .expect("the journey answers the read");
    runtime.ingest(&frame(reply), &Mapper).unwrap();
    request
}

fn skills(runtime: &CodexRuntime) -> Vec<String> {
    let list = runtime.available_commands().snapshot();
    list.commands
        .iter()
        .filter(|command| command.source == CommandSource::Skill)
        .map(|command| command.name.clone())
        .collect()
}

#[test]
fn the_skills_journey_lists_enabled_skills_and_reloads_them_after_a_change() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let first = read(&mut runtime, None);
    assert_eq!(first["params"]["cwds"], json!([r"C:\repo"]));
    assert!(
        first["params"].get("forceReload").is_none(),
        "Codex's cache serves the first read"
    );
    assert_eq!(
        skills(&runtime),
        ["pdf", LONG],
        "the disabled skill is left out"
    );
    let changed = journey(std::slice::from_ref(&first), plain_turn(), &[])
        .into_iter()
        .find(|frame| frame["method"] == "skills/changed")
        .expect("Codex says the skills changed");
    runtime.ingest(&frame(changed), &Mapper).unwrap();
    let reload = read(&mut runtime, Some(&first));
    assert_eq!(reload["params"]["forceReload"], true);
    assert_eq!(skills(&runtime), ["pdf", LONG, "changelog"]);
}

#[test]
fn a_picked_skill_reaches_the_journeys_turn_as_its_skill_input() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let first = read(&mut runtime, None);
    let mut prompt = Prompt::new("prompt", "/pdf summarise it").unwrap();
    prompt.command_revision = Some(runtime.available_commands().snapshot().revision);
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt,
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: sage_acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let mut turn = wire(&egress);
    turn["id"] = json!(104);
    let answer = journey(&[first], turn, &[])
        .into_iter()
        .find(|frame| frame["method"] == "item/completed")
        .expect("the turn answers");
    let echo = answer["params"]["item"]["text"].as_str().unwrap();
    let blocks: Vec<&str> = echo.split("\n\n").collect();
    assert_eq!(blocks.len(), 3, "{echo}");
    assert_eq!(blocks[0], "Synthetic replay received 2 prompt blocks:");
    let skill_path = blocks[1].strip_prefix("Skill pdf at ").expect(echo);
    let expected_path = std::env::current_dir()
        .unwrap()
        .join(".codex")
        .join("skills")
        .join("pdf")
        .join("SKILL.md");
    assert_eq!(
        std::path::Path::new(skill_path),
        expected_path.as_path(),
        "{echo}"
    );
    assert_eq!(blocks[2], "Text:\n```text\nsummarise it\n```");
}
