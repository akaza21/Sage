use super::*;
use crate::acp::slash_commands::CommandSource;

fn skills_reply(id: u64, skills: Value) -> Frame {
    frame(json!({"id":id,"result":{"data":[{"cwd":r"C:\repo","errors":[],"skills":skills}]}}))
}

fn skill(name: &str, enabled: bool) -> Value {
    json!({"name":name,"description":format!("{name} in full"),"enabled":enabled,
        "path":format!(r"C:\skills\{name}\SKILL.md"),"scope":"user"})
}

fn listed(runtime: &CodexRuntime) -> Vec<String> {
    let list = runtime.available_commands().snapshot();
    list.commands.iter().map(|c| c.name.clone()).collect()
}

fn read_skills(runtime: &mut CodexRuntime, skills: Value) {
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(wire["method"], "skills/list");
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    // One read at a time.
    assert!(runtime.discover_commands().unwrap().is_none());
    let batch = runtime
        .ingest(&skills_reply(wire["id"].as_u64().unwrap(), skills), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn enabled_skills_list_as_commands_and_are_read_again_when_codex_says_they_changed() {
    let mut runtime = fixture_runtime();
    assert!(runtime.discover_commands().unwrap().is_none());
    ready(&mut runtime);
    let invalid =
        json!({"name":"two words","description":"d","enabled":true,"path":"p","scope":"repo"});
    let mut short = skill("pdf", true);
    short["interface"] = json!({"shortDescription":"Read PDFs"});
    read_skills(
        &mut runtime,
        json!([
            short,
            skill("off", false),
            invalid,
            skill("pdf", true),
            skill("lint", true)
        ]),
    );
    let list = runtime.available_commands().snapshot();
    assert_eq!(listed(&runtime), ["pdf", "lint"]);
    assert_eq!(list.commands[0].description, "Read PDFs");
    assert_eq!(list.commands[1].description, "lint in full");
    assert!(
        list.commands
            .iter()
            .all(|c| c.source == CommandSource::Skill)
    );
    // Nothing more to read until Codex says the skills changed.
    assert!(runtime.discover_commands().unwrap().is_none());
    runtime
        .ingest(
            &frame(json!({"method":"skills/changed","params":{}})),
            &Mapper,
        )
        .unwrap();
    read_skills(&mut runtime, json!([skill("lint", true)]));
    assert_eq!(listed(&runtime), ["lint"]);
    assert!(runtime.available_commands().snapshot().revision > list.revision);
}

#[test]
fn a_listed_skill_runs_with_the_skill_attached() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    read_skills(&mut runtime, json!([skill("pdf", true)]));
    let revision = runtime.available_commands().snapshot().revision;
    let input = |runtime: &mut CodexRuntime, text: &str, revision: Option<u64>| {
        let mut prompt = Prompt::new("prompt", text).unwrap();
        prompt.command_revision = revision;
        let egress = runtime
            .command(AdapterCommand::Prompt {
                prompt,
                client_message_id: RequiredText::new("client", "client-1").unwrap(),
                access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
            })
            .unwrap();
        let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
        runtime
            .resolve_egress(egress.into_token(), EgressResolution::Queued)
            .unwrap();
        wire["params"]["input"].clone()
    };
    assert_eq!(
        input(&mut runtime, "/pdf summarise it", Some(revision)),
        json!([
            {"type":"skill","name":"pdf","path":r"C:\skills\pdf\SKILL.md"},
            {"type":"text","text":"/pdf summarise it"}
        ])
    );
}

#[test]
fn typed_text_naming_a_skill_is_only_text() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    read_skills(&mut runtime, json!([skill("pdf", true)]));
    let egress = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "/pdf summarise it").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let wire: Value = serde_json::from_slice(egress.frame().payload()).unwrap();
    assert_eq!(
        wire["params"]["input"],
        json!([{"type":"text","text":"/pdf summarise it"}])
    );
}

#[test]
fn a_refused_skills_read_leaves_an_empty_list_and_is_not_retried() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32601,"message":"unknown method"}})),
            &Mapper,
        )
        .unwrap();
    assert!(listed(&runtime).is_empty());
    assert!(runtime.discover_commands().unwrap().is_none());
}
