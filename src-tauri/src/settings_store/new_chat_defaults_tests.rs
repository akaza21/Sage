use super::*;

fn defaults(harness: Option<&str>, model: Option<&str>, effort: Option<&str>) -> NewChatDefaults {
    NewChatDefaults {
        harness_id: harness.map(Into::into),
        model_id: model.map(Into::into),
        effort: effort.map(Into::into),
    }
}

#[test]
fn a_fresh_store_has_no_new_chat_defaults() {
    let store = SettingsStore::in_memory().unwrap();
    assert_eq!(
        store.new_chat_defaults().unwrap(),
        NewChatDefaults::default()
    );
}

#[test]
fn new_chat_defaults_survive_reopening_the_store() {
    let directory =
        std::env::temp_dir().join(format!("sage-settings-new-chat-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("settings.db");
    let value = defaults(Some("codex"), Some("gpt-5.5"), Some("high"));
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .set_new_chat_defaults(value.clone())
            .unwrap(),
        value
    );
    assert_eq!(
        SettingsStore::open(&path)
            .unwrap()
            .new_chat_defaults()
            .unwrap(),
        value
    );
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn saving_another_harness_drops_the_model_and_effort_picked_for_the_previous_one() {
    let store = SettingsStore::in_memory().unwrap();
    store
        .set_new_chat_defaults(defaults(Some("codex"), Some("gpt-5.5"), Some("high")))
        .unwrap();
    store
        .set_new_chat_defaults(defaults(Some("claude-code"), None, None))
        .unwrap();
    assert_eq!(
        store.new_chat_defaults().unwrap(),
        defaults(Some("claude-code"), None, None)
    );
}

#[test]
fn saving_no_harness_clears_the_defaults() {
    let store = SettingsStore::in_memory().unwrap();
    store
        .set_new_chat_defaults(defaults(Some("pi"), Some("sonnet"), None))
        .unwrap();
    store
        .set_new_chat_defaults(NewChatDefaults::default())
        .unwrap();
    assert_eq!(
        store.new_chat_defaults().unwrap(),
        NewChatDefaults::default()
    );
}

#[test]
fn malformed_defaults_are_refused_and_leave_the_stored_record_untouched() {
    let store = SettingsStore::in_memory().unwrap();
    let kept = defaults(Some("codex"), Some("gpt-5.5"), None);
    store.set_new_chat_defaults(kept.clone()).unwrap();
    let too_long = "m".repeat(MAX_NEW_CHAT_DEFAULT_BYTES + 1);
    for refused in [
        defaults(None, Some("gpt-5.5"), None),
        defaults(None, None, Some("high")),
        defaults(Some(""), None, None),
        defaults(Some(" codex"), None, None),
        defaults(Some("codex"), Some("gpt\n5"), None),
        defaults(Some("codex"), Some(&too_long), None),
        defaults(Some("codex"), None, Some("")),
    ] {
        assert!(
            store.set_new_chat_defaults(refused.clone()).is_err(),
            "{refused:?} was admitted"
        );
    }
    assert_eq!(store.new_chat_defaults().unwrap(), kept);
}

#[test]
fn the_longest_admitted_identifier_is_stored_whole() {
    let store = SettingsStore::in_memory().unwrap();
    // Multi-byte text measures the SQLite bound in bytes, like the Rust one.
    let model = "é".repeat(MAX_NEW_CHAT_DEFAULT_BYTES / 2);
    let value = defaults(Some("codex"), Some(&model), None);
    store.set_new_chat_defaults(value.clone()).unwrap();
    assert_eq!(store.new_chat_defaults().unwrap(), value);
}

#[test]
fn new_chat_defaults_refuse_unknown_fields() {
    assert!(
        serde_json::from_str::<NewChatDefaults>(r#"{"harnessId":"codex","path":"x"}"#).is_err()
    );
    assert_eq!(
        serde_json::from_str::<NewChatDefaults>(r#"{"harnessId":"codex"}"#).unwrap(),
        defaults(Some("codex"), None, None)
    );
}
