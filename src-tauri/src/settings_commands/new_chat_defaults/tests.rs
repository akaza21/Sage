use super::*;
use crate::harnesses::SIMULATOR_ID;

fn defaults(harness: &str, model: Option<&str>, effort: Option<&str>) -> NewChatDefaults {
    NewChatDefaults {
        harness_id: Some(harness.into()),
        model_id: model.map(Into::into),
        effort: effort.map(Into::into),
    }
}

/// Discovery has settled and found nothing installed; only the development
/// simulator, which the debug catalog always admits, can run.
fn settled_catalog() -> HarnessCatalog {
    HarnessCatalog::default().settled_for_test()
}

#[test]
fn an_available_harness_is_saved_with_its_model_and_effort() {
    let settings = SettingsStore::in_memory().unwrap();
    let catalog = settled_catalog();
    let value = defaults(SIMULATOR_ID, Some("sim-deep"), Some("high"));
    let saved = save_new_chat_defaults(&settings, &catalog, value.clone()).unwrap();
    assert_eq!(
        saved,
        NewChatDefaultsState {
            defaults: value.clone(),
            harness_status: Some(DefaultHarnessStatus::Available),
            model_status: Some(DefaultOptionStatus::Available),
            // No discovery source lists efforts; the live session judges it.
            effort_status: Some(DefaultOptionStatus::Unknown),
        }
    );
    assert_eq!(read_new_chat_defaults(&settings, &catalog).unwrap(), saved);
}

#[test]
fn a_model_the_harness_does_not_offer_is_reported_unavailable_and_kept() {
    let settings = SettingsStore::in_memory().unwrap();
    let catalog = settled_catalog();
    let value = defaults(SIMULATOR_ID, Some("retired-model"), None);
    let saved = save_new_chat_defaults(&settings, &catalog, value.clone()).unwrap();
    assert_eq!(saved.model_status, Some(DefaultOptionStatus::Unavailable));
    assert_eq!(settings.new_chat_defaults().unwrap(), value);
}

#[test]
fn a_harness_outside_the_catalog_is_refused_and_the_stored_default_kept() {
    let settings = SettingsStore::in_memory().unwrap();
    let catalog = settled_catalog();
    let kept = defaults(SIMULATOR_ID, None, None);
    save_new_chat_defaults(&settings, &catalog, kept.clone()).unwrap();
    assert!(
        save_new_chat_defaults(&settings, &catalog, defaults("not-a-harness", None, None)).is_err()
    );
    assert_eq!(settings.new_chat_defaults().unwrap(), kept);
}

#[test]
fn a_harness_discovery_did_not_find_is_refused() {
    let settings = SettingsStore::in_memory().unwrap();
    let error = save_new_chat_defaults(&settings, &settled_catalog(), defaults("pi", None, None))
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidRequest(_)));
    assert_eq!(
        settings.new_chat_defaults().unwrap(),
        NewChatDefaults::default()
    );
}

#[test]
fn a_harness_saved_during_discovery_is_kept_and_reported_once_discovery_settles() {
    let settings = SettingsStore::in_memory().unwrap();
    let detecting = HarnessCatalog::default();
    let value = defaults("pi", Some("sonnet"), None);
    assert_eq!(
        save_new_chat_defaults(&settings, &detecting, value.clone())
            .unwrap()
            .harness_status,
        Some(DefaultHarnessStatus::Detecting)
    );
    let saved = save_new_chat_defaults(&settings, &detecting, value.clone()).unwrap();
    // Nothing can judge the model before discovery settles.
    assert_eq!(saved.model_status, Some(DefaultOptionStatus::Unknown));
    // Discovery finds no Pi: the default is reported, never rewritten.
    assert_eq!(
        read_new_chat_defaults(&settings, &settled_catalog()).unwrap(),
        NewChatDefaultsState {
            defaults: value,
            harness_status: Some(DefaultHarnessStatus::NotInstalled),
            model_status: Some(DefaultOptionStatus::Unknown),
            effort_status: None,
        }
    );
}

#[test]
fn a_stored_harness_that_left_the_catalog_is_reported_as_unknown() {
    let settings = SettingsStore::in_memory().unwrap();
    let retired = defaults("retired-harness", None, None);
    settings.set_new_chat_defaults(retired.clone()).unwrap();
    assert_eq!(
        read_new_chat_defaults(&settings, &settled_catalog()).unwrap(),
        NewChatDefaultsState {
            defaults: retired,
            harness_status: Some(DefaultHarnessStatus::Unknown),
            model_status: None,
            effort_status: None,
        }
    );
}

#[test]
fn clearing_the_defaults_reports_no_harness() {
    let settings = SettingsStore::in_memory().unwrap();
    let catalog = settled_catalog();
    save_new_chat_defaults(&settings, &catalog, defaults(SIMULATOR_ID, None, None)).unwrap();
    assert_eq!(
        save_new_chat_defaults(&settings, &catalog, NewChatDefaults::default()).unwrap(),
        NewChatDefaultsState {
            defaults: NewChatDefaults::default(),
            harness_status: None,
            model_status: None,
            effort_status: None,
        }
    );
}

#[test]
fn the_state_serializes_for_the_settings_port() {
    let state = NewChatDefaultsState {
        defaults: defaults("codex", Some("gpt-5.5"), None),
        harness_status: Some(DefaultHarnessStatus::NotInstalled),
        model_status: Some(DefaultOptionStatus::Unknown),
        effort_status: None,
    };
    assert_eq!(
        serde_json::to_value(state).unwrap(),
        serde_json::json!({
            "defaults": { "harnessId": "codex", "modelId": "gpt-5.5", "effort": null },
            "harnessStatus": "notInstalled",
            "modelStatus": "unknown",
            "effortStatus": null,
        })
    );
}
