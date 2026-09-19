use super::*;
use serde_json::json;

fn owner(epoch: u64) -> crate::acp::scheduler::RuntimeOwner {
    use crate::acp::contract::*;
    crate::acp::scheduler::RuntimeOwner {
        project_id: SageProjectId::parse("project-a").unwrap(),
        chat_id: SageChatId::parse("chat-a").unwrap(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
    }
}
#[test]
fn overlay_is_ephemeral_exact_owner_monotonic_and_coalesced() {
    let channel = ConfigChannel::default();
    let owner = owner(1);
    channel.reserve(owner.clone());
    let config = SessionConfig::from_wire(&json!({"configOptions":[select()]}), true).unwrap();
    channel.update(&owner, config.clone()).unwrap();
    assert!(
        channel.snapshot(&owner).is_none(),
        "activation has not published ownership"
    );
    assert!(channel.take_notice().is_none());
    channel.publish(&owner);
    assert_eq!(channel.snapshot(&owner).unwrap().config_revision.get(), 1);
    channel.update(&owner, SessionConfig::default()).unwrap();
    channel.update(&owner, config).unwrap();
    assert_eq!(channel.take_notice().unwrap().config_revision.get(), 3);
    assert!(channel.take_notice().is_none());
    channel.remove(&owner).unwrap();
    let tombstone = channel.snapshot(&owner).unwrap();
    assert_eq!(tombstone.config_revision.get(), 4);
    assert!(tombstone.removed);
    assert!(channel.live_snapshot(&owner).is_none());
    channel
        .update(
            &owner,
            SessionConfig::from_wire(&json!({"configOptions":[select()]}), true).unwrap(),
        )
        .unwrap();
    let retained = channel.snapshot(&owner).unwrap();
    assert_eq!(retained.config_revision.get(), 4);
    assert!(
        retained.removed,
        "a delayed update cannot resurrect a tombstoned owner"
    );
    let mut newer = owner.clone();
    newer.runtime_epoch = crate::acp::contract::RuntimeEpoch::new(2).unwrap();
    channel.reserve(newer.clone());
    channel.update(&newer, SessionConfig::default()).unwrap();
    channel.publish(&newer);
    channel.clear(&owner);
    assert_eq!(channel.snapshot(&newer).unwrap().config_revision.get(), 1);
    assert!(channel.snapshot(&owner).is_none());
    channel.clear(&newer);
    channel.update(&newer, SessionConfig::default()).unwrap();
    assert!(
        channel.snapshot(&newer).is_none(),
        "late updates cannot resurrect a released owner"
    );
    assert!(
        ConfigChannel::default().snapshot(&newer).is_none(),
        "restart has no overlay"
    );
}
#[test]
fn overlay_validates_exact_revision_before_advertised_value() {
    let channel = ConfigChannel::default();
    let owner = owner(1);
    channel.reserve(owner.clone());
    channel
        .update(
            &owner,
            SessionConfig::from_wire(&json!({"configOptions":[select()]}), true).unwrap(),
        )
        .unwrap();
    channel.publish(&owner);
    let revision = channel.live_snapshot(&owner).unwrap().config_revision;
    let mut intent = ConfigIntent {
        owner: owner.clone(),
        expected_config_revision: revision,
        mutation: ConfigMutation::Option {
            config_id: ConfigId::new("id", "effort").unwrap(),
            value: ConfigValue::Select {
                value: ConfigId::new("value", "high").unwrap(),
            },
        },
    };
    assert!(channel.validate(&intent).is_ok());
    intent.owner.runtime_epoch = crate::acp::contract::RuntimeEpoch::new(2).unwrap();
    assert!(channel.validate(&intent).is_err());
    intent.owner = owner.clone();
    channel.remove(&owner).unwrap();
    assert!(channel.snapshot(&owner).unwrap().removed);
    assert!(channel.validate(&intent).is_err());
    channel.clear_all();
    assert!(channel.snapshot(&owner).is_none());
}

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
    assert!(SessionConfig::from_wire(&grouped, false).is_err());
}

#[test]
fn malformed_duplicate_and_over_budget_config_fail_closed() {
    for value in [
        json!({"modes":{"currentModeId":"missing","availableModes":[{"id":"ask","name":"Ask"}]}}),
        json!({"configOptions":[select(),select()]}),
        json!({"configOptions":[{"id":"x","name":"X","type":"text","currentValue":"a"}]}),
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
