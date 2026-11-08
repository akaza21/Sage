use super::*;

fn launched_in(dir: &std::path::Path) -> PiRuntime {
    let mut runtime = fixture_runtime(95);
    initialize_with(
        &mut runtime,
        json!({"model":{"provider":"openai","id":"gpt","name":"GPT"},"thinkingLevel":"high",
            "sessionId":"launch","sessionFile":dir.join("9_launch.jsonl")}),
    );
    runtime
}

fn resume(runtime: &mut PiRuntime, id: &str) -> Result<AdapterEgress, AdapterRuntimeError> {
    runtime.command(AdapterCommand::ResumeSession {
        external_session_id: ExternalId::new("session", id).unwrap(),
        native_root: r"C:\repo".into(),
        model: None,
    })
}

fn sessions_dir() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("sage-pi-sessions-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn a_new_chat_is_known_by_pis_own_session_id() {
    let mut runtime = fixture_runtime(94);
    initialize(&mut runtime);
    let session = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    // Pi opened a fresh session at launch; the chat takes it by its id.
    let value = deliver(&mut runtime, session);
    assert_eq!(value["type"], "get_state");
    let batch = runtime
        .ingest(&response(&value, opened()), &Mapper)
        .unwrap();
    assert!(
        matches!(batch.events(), [AdapterEvent::SessionReady { id, loaded: false }]
        if id.external_id.as_str() == "pi-session-1")
    );
}

#[test]
fn a_saved_chat_reopens_from_pis_session_file() {
    let dir = sessions_dir();
    let id = "019a2b3c-4d5e-7f00-8a9b-0c1d2e3f4a5b";
    let file = dir.join(format!("2026-11-08T10-00-00-000Z_{id}.jsonl"));
    std::fs::write(&file, "{}\n").unwrap();
    std::fs::write(dir.join("2026-11-08T09-00-00-000Z_other.jsonl"), "{}\n").unwrap();
    let mut runtime = launched_in(&dir);
    let switch = resume(&mut runtime, id).unwrap();
    let value = deliver(&mut runtime, switch);
    assert_eq!(value["type"], "switch_session");
    assert_eq!(value["sessionPath"], file.to_str().unwrap());
    let batch = runtime
        .ingest(&response(&value, json!({"cancelled":false})), &Mapper)
        .unwrap();
    assert!(
        matches!(batch.events(), [AdapterEvent::SessionReady { id: ready, loaded: false }]
        if ready.external_id.as_str() == id)
    );
    // The reopened chat takes prompts like a new one.
    running(&mut runtime);
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_chat_pi_never_saved_or_a_foreign_id_does_not_reopen() {
    let dir = sessions_dir();
    std::fs::write(dir.join("1_saved.jsonl"), "{}\n").unwrap();
    let mut runtime = launched_in(&dir);
    for id in ["unsaved", "../1_saved", "1_saved.jsonl", "saved/x"] {
        assert!(
            matches!(
                resume(&mut runtime, id),
                Err(AdapterRuntimeError::UnsupportedCommand)
            ),
            "{id}"
        );
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_switch_an_extension_cancels_fails_the_reopen() {
    let dir = sessions_dir();
    std::fs::write(dir.join("1_saved.jsonl"), "{}\n").unwrap();
    let mut runtime = launched_in(&dir);
    let switch = resume(&mut runtime, "saved").unwrap();
    let value = deliver(&mut runtime, switch);
    let batch = runtime
        .ingest(&response(&value, json!({"cancelled":true})), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Error(_)]));
    assert!(runtime.external_session.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn a_chat_reopens_after_pis_default_model_changed() {
    let dir = sessions_dir();
    std::fs::write(dir.join("1_saved.jsonl"), "{}\n").unwrap();
    let mut runtime = launched_in(&dir);
    let durable = "anthropic:claude";
    assert_ne!(runtime.current_model.as_deref(), Some(durable));
    let switch = runtime
        .command(AdapterCommand::ResumeSession {
            external_session_id: ExternalId::new("session", "saved").unwrap(),
            native_root: r"C:\repo".into(),
            model: Some(RequiredText::new("model", durable).unwrap()),
        })
        .unwrap();
    let value = deliver(&mut runtime, switch);
    assert_eq!(value["type"], "switch_session");
    std::fs::remove_dir_all(dir).unwrap();
}
