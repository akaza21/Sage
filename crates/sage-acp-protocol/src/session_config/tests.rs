use super::*;
#[test]
fn wire_defaults_are_tolerated_but_never_confirmed_by_generic_acp() {
    let option = json!({"id":"effort","name":"Effort","type":"select",
        "currentValue":"high","defaultValue":"low",
        "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]});
    for default in [
        json!("low"),
        json!("unsupported"),
        json!(""),
        json!(null),
        json!(1),
        json!(true),
    ] {
        let mut invalid = option.clone();
        invalid["defaultValue"] = default;
        let config = SessionConfig::from_wire(&json!({"configOptions":[invalid,
            {"id":"flag","name":"Flag","type":"boolean","currentValue":true,"defaultValue":false}
        ]}), true).unwrap();
        let wire = serde_json::to_value(config).unwrap();
        assert_eq!(wire["configOptions"].as_array().unwrap().len(), 2);
        for option in wire["configOptions"].as_array().unwrap() {
            assert!(option.get("defaultValue").is_none());
        }
    }
    let mut absent = option;
    absent.as_object_mut().unwrap().remove("defaultValue");
    let config = SessionConfig::from_wire(&json!({"configOptions":[absent]}), true).unwrap();
    assert!(
        serde_json::to_value(config).unwrap()["configOptions"][0]
            .get("defaultValue")
            .is_none()
    );
}
#[test]
fn generic_acp_preserves_thought_level_category() {
    let config = SessionConfig::from_wire(&json!({"configOptions":[select()]}), true).unwrap();
    assert_eq!(
        serde_json::to_value(config).unwrap()["configOptions"][0]["category"],
        "thought_level"
    );
}
#[test]
fn confirmed_effort_is_bounded_binding_metadata_without_selectable_authority() {
    let mut config =
        SessionConfig::from_wire(&json!({"configOptions":[],"confirmedEffort":"high"}), true)
            .unwrap();
    assert!(config.confirmed_effort.is_none());
    config.confirmed_effort = Some(RequiredText::new("reasoningEffort", "high").unwrap());
    assert!(config.validate().is_ok());
    assert!(
        config
            .validate_value(
                "effort",
                &ConfigValue::Select {
                    value: ConfigId::new("value", "high").unwrap()
                }
            )
            .is_err()
    );
    assert!(
        serde_json::from_value::<SessionConfig>(json!({
            "confirmedEffort":"x".repeat(513)
        }))
        .is_err()
    );
    let options: Vec<_> = (0..128)
        .map(|index| {
            json!({
                "id":format!("flag-{index}"),"name":"Flag","type":"boolean","currentValue":false
            })
        })
        .collect();
    config.config_options = SessionConfig::from_wire(&json!({"configOptions":options}), true)
        .unwrap()
        .config_options;
    assert!(config.validate().is_err());
}

#[test]
fn confirmed_model_is_bounded_binding_metadata_not_a_selectable_acp_extension() {
    let supplied = json!({"configOptions":[],"confirmedModel":{"id":"actual","name":"Actual"}});
    assert!(
        SessionConfig::from_wire(&supplied, true)
            .unwrap()
            .confirmed_model
            .is_none()
    );
    let mut config = SessionConfig {
        confirmed_model: Some(ConfirmedModel {
            id: RequiredText::new("modelId", "actual").unwrap(),
            name: Some(RequiredText::new("modelName", "Actual").unwrap()),
        }),
        ..Default::default()
    };
    assert!(config.validate().is_ok());
    assert!(
        config
            .validate_value(
                "model",
                &ConfigValue::Select {
                    value: ConfigId::new("value", "actual").unwrap()
                }
            )
            .is_err()
    );
    assert!(serde_json::from_value::<ConfirmedModel>(json!({"id":"x".repeat(513)})).is_err());
    let options: Vec<_> = (0..128).map(|index| json!({"id":format!("flag-{index}"),"name":"Flag","type":"boolean","currentValue":false})).collect();
    config.config_options = SessionConfig::from_wire(&json!({"configOptions":options}), true)
        .unwrap()
        .config_options;
    assert!(config.validate().is_err());
}

use serde_json::json;

fn select() -> serde_json::Value {
    json!({"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":"high","options":[{"value":"high","name":"High"}]})
}

#[test]
fn initial_and_replacement_config_preserve_v1_forms() {
    let state = SessionConfig::from_wire(&json!({"modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"}]},"configOptions":[select()]}), true).unwrap();
    assert_eq!(state.config_options.as_slice().len(), 1);
    assert_eq!(
        state.modes.as_ref().unwrap().current_mode_id.as_str(),
        "ask"
    );
    let grouped = json!({"configOptions":[{"id":"model","name":"Model","type":"select","currentValue":"a","options":[{"group":"models","name":"Models","options":[{"value":"a","name":"A"}]}]},{"id":"flag","name":"Flag","type":"boolean","currentValue":false}]});
    assert_eq!(
        SessionConfig::from_wire(&grouped, true)
            .unwrap()
            .config_options
            .as_slice()
            .len(),
        2
    );
    let demoted = SessionConfig::from_wire(&grouped, false).unwrap();
    assert_eq!(demoted.config_options.as_slice().len(), 2);
    let ConfigKind::Unsupported { kind } = &demoted.config_options.as_slice()[1].kind else {
        panic!("an unnegotiated boolean must survive as an unsupported option")
    };
    assert_eq!(kind.as_str(), "boolean");
    assert!(matches!(
        demoted.config_options.as_slice()[0].kind,
        ConfigKind::Select { .. }
    ));
}

#[test]
fn a_demoted_boolean_projects_as_unsupported_and_never_as_a_raw_boolean() {
    let wire =
        json!({"configOptions":[{"id":"flag","name":"Flag","type":"boolean","currentValue":true}]});
    let projected = serde_json::to_value(SessionConfig::from_wire(&wire, false).unwrap()).unwrap();
    let option = &projected["configOptions"][0];
    assert_eq!(option["type"], json!("unsupported"));
    assert_eq!(option["kind"], json!("boolean"));
    assert_eq!(option["id"], json!("flag"));
    assert_eq!(option["name"], json!("Flag"));
    assert!(option.get("currentValue").is_none());
}

#[test]
fn a_projected_unsupported_option_reparses_with_its_original_kind() {
    let wire =
        json!({"configOptions":[{"id":"flag","name":"Flag","type":"boolean","currentValue":true}]});
    let projected = serde_json::to_value(SessionConfig::from_wire(&wire, false).unwrap()).unwrap();
    let reparsed = SessionConfig::from_wire(&projected, false).unwrap();
    let ConfigKind::Unsupported { kind } = &reparsed.config_options.as_slice()[0].kind else {
        panic!("a projected unsupported option must reparse as unsupported")
    };
    assert_eq!(kind.as_str(), "boolean");
}

#[test]
fn an_unknown_option_type_degrades_without_discarding_its_neighbours() {
    let unknown =
        json!({"id":"x","name":"X","type":"text","currentValue":"a","placeholder":"type here"});
    let config =
        SessionConfig::from_wire(&json!({"configOptions":[select(), unknown.clone()]}), true)
            .unwrap();
    assert!(matches!(
        config.config_options.as_slice()[0].kind,
        ConfigKind::Select { .. }
    ));
    let option = &config.config_options.as_slice()[1];
    let ConfigKind::Unsupported { kind, .. } = &option.kind else {
        panic!("an unknown option type must degrade rather than fail the config")
    };
    assert_eq!(kind.as_str(), "text");
    assert_eq!(
        serde_json::to_value(option).unwrap(),
        json!({"id":"x","name":"X","type":"unsupported","kind":"text"})
    );
}

#[test]
fn an_unsupported_option_is_never_settable() {
    let config = SessionConfig::from_wire(
        &json!({"configOptions":[{"id":"x","name":"X","type":"text","currentValue":"a"}]}),
        true,
    )
    .unwrap();
    assert_eq!(
        config.validate_value(
            "x",
            &ConfigValue::Select {
                value: ConfigId::new("value", "a").unwrap()
            }
        ),
        Err("session config value not advertised".into())
    );
}

#[test]
fn opencode_session_configuration_decodes_from_its_captured_response() {
    let frames = include_str!(
        "../../../../src-tauri/tests/fixtures/harness-traces/opencode/tool-kinds/frames.jsonl"
    );
    let response = frames
        .lines()
        .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .find(|frame| frame["frame"]["id"] == json!(2) && frame["frame"]["result"].is_object())
        .expect("opencode session/new response");
    let config = SessionConfig::from_wire(&response["frame"]["result"], true).unwrap();
    let ids = config
        .config_options
        .as_slice()
        .iter()
        .map(|option| option.id.as_str())
        .collect::<Vec<_>>();
    assert_eq!(ids, ["model", "mode"]);
    assert!(
        config
            .config_options
            .as_slice()
            .iter()
            .all(|option| matches!(option.kind, ConfigKind::Select { .. }))
    );
}

#[test]
fn malformed_duplicate_and_over_budget_config_fail_closed() {
    for value in [
        json!({"modes":{"currentModeId":"missing","availableModes":[{"id":"ask","name":"Ask"}]}}),
        json!({"configOptions":[select(),select()]}),
        json!({"configOptions":[{"id":"x","name":"X","type":"select","currentValue":"a","options":[{"value":"b","name":"B"}]}]}),
        json!({"configOptions":[{"id":"x","name":"X","type":"select","currentValue":"a","options":[{"value":"a","name":"A"},{"value":"a","name":"Again"}]}]}),
        json!({"configOptions":[{"id":"x","name":"X","type":"select","currentValue":"a","options":[{"value":"a","name":"A"},{"group":"g","name":"G","options":[]}]}]}),
        json!({"configOptions":vec![select();129]}),
        json!({"modes":{"currentModeId":"a","availableModes":[{"id":"a","name":"x".repeat(131073)}]}}),
    ] {
        assert!(
            SessionConfig::from_wire(&value, true).is_err(),
            "accepted {value}"
        );
    }
}

#[test]
fn config_budget_counts_only_admitted_canonical_fields() {
    let mut option = select();
    option["vendorExtension"] = json!("x".repeat(131_073));
    assert!(SessionConfig::from_wire(&json!({"configOptions":[option]}), true).is_ok());

    let admitted = (0..8)
        .map(|index| {
            json!({
                "id": format!("flag-{index}"),
                "name": "Flag",
                "description": "x".repeat(16_384),
                "type": "boolean",
                "currentValue": false
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        SessionConfig::from_wire(&json!({"configOptions": admitted}), true),
        Err("session config byte limit exceeded".into())
    );
}

#[test]
fn config_value_accepts_v1_string_fallback_and_rejects_wrong_shapes() {
    for value in [
        json!({"value":"high"}),
        json!({"type":"select","value":"high"}),
        json!({"type":"text","value":"high"}),
        json!({"type":"vendor_kind","value":"high"}),
    ] {
        assert_eq!(
            serde_json::from_value::<ConfigValue>(value).unwrap(),
            ConfigValue::Select {
                value: ConfigId::new("value", "high").unwrap()
            }
        );
    }
    assert_eq!(
        serde_json::from_value::<ConfigValue>(json!({"type":"boolean","value":true})).unwrap(),
        ConfigValue::Boolean { value: true }
    );
    for value in [
        json!({"type":"boolean","value":"high"}),
        json!({"type":"text","value":true}),
        json!({"type":"vendor_kind","value":1}),
    ] {
        assert!(serde_json::from_value::<ConfigValue>(value).is_err());
    }
}

#[test]
fn only_advertised_values_are_valid_and_current_mode_is_authoritative() {
    let mut state = SessionConfig::from_wire(&json!({"modes":{"currentModeId":"ask","availableModes":[{"id":"ask","name":"Ask"},{"id":"code","name":"Code"}]},"configOptions":[select()]}), true).unwrap();
    assert!(state.validate_mode("code").is_ok());
    assert_eq!(
        state.modes.as_ref().unwrap().current_mode_id.as_str(),
        "ask"
    );
    assert!(
        state
            .validate_value(
                "effort",
                &ConfigValue::Select {
                    value: ConfigId::new("value", "high").unwrap()
                }
            )
            .is_ok()
    );
    assert!(
        state
            .validate_value("effort", &ConfigValue::Boolean { value: true })
            .is_err()
    );
    assert!(
        state
            .set_current_mode(ConfigId::new("mode", "missing").unwrap())
            .is_err()
    );
    state
        .set_current_mode(ConfigId::new("mode", "code").unwrap())
        .unwrap();
    assert_eq!(state.modes.unwrap().current_mode_id.as_str(), "code");
}
