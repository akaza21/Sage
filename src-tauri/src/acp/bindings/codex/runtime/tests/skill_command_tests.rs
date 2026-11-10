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

fn skills_changed(runtime: &mut CodexRuntime) {
    runtime
        .ingest(
            &frame(json!({"method":"skills/changed","params":{}})),
            &Mapper,
        )
        .unwrap();
}

#[test]
fn an_unchanged_reread_keeps_the_revision_and_a_refused_one_empties_the_list() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    read_skills(&mut runtime, json!([skill("pdf", true)]));
    let revision = runtime.available_commands().snapshot().revision;
    // A pick made from the shown list still runs after an equal re-read.
    skills_changed(&mut runtime);
    read_skills(&mut runtime, json!([skill("pdf", true)]));
    assert_eq!(runtime.available_commands().snapshot().revision, revision);
    // A refused re-read drops the list: a listed path may no longer exist.
    skills_changed(&mut runtime);
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32603,"message":"failed"}})),
            &Mapper,
        )
        .unwrap();
    assert!(listed(&runtime).is_empty());
    assert!(runtime.available_commands().snapshot().revision > revision);
}

#[test]
fn an_oversized_skills_reply_is_a_refused_read_not_a_broken_session() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    read_skills(&mut runtime, json!([skill("pdf", true)]));
    skills_changed(&mut runtime);
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    let id = request_id(&egress);
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    // The pump decodes oversize replies recoverably while the read is out.
    assert!(runtime.child_read_pending());
    runtime.accept_oversized_response(id).unwrap();
    assert!(!runtime.child_read_pending());
    assert!(listed(&runtime).is_empty());
    // The session still runs prompts.
    runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "hello").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
}

#[test]
fn a_long_catalogue_lists_the_skills_that_fit_the_byte_bound() {
    use crate::acp::bindings::standard_acp::surface::command_list::MAX_COMMAND_BYTES;
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let skills: Vec<Value> = (0..400)
        .map(|index| {
            let mut entry = skill(&format!("skill-{index}"), true);
            entry["description"] = json!("d".repeat(1000));
            entry
        })
        .collect();
    read_skills(&mut runtime, Value::Array(skills));
    let list = runtime.available_commands().snapshot();
    let bytes: usize = list.commands.iter().map(|c| c.listed_bytes()).sum();
    // Every skill that fits is listed, in order; the next one would not fit.
    assert!(bytes <= MAX_COMMAND_BYTES);
    assert!(bytes + 1000 + "skill-399".len() > MAX_COMMAND_BYTES);
    let names: Vec<String> = (0..list.commands.len())
        .map(|index| format!("skill-{index}"))
        .collect();
    assert_eq!(listed(&runtime), names);
}

#[test]
fn a_skills_read_that_never_left_is_wanted_again() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let egress = runtime.discover_commands().unwrap().expect("skills/list");
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(runtime.discover_commands().unwrap().is_some());
}
