use super::*;
use sage_acp_protocol::session_config::{ConfigId, ConfigValue};
use serde_json::json;

fn owner(epoch: u64) -> sage_acp_protocol::scheduler::RuntimeOwner {
    use sage_acp_protocol::contract::*;
    sage_acp_protocol::scheduler::RuntimeOwner {
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
    newer.runtime_epoch = sage_acp_protocol::contract::RuntimeEpoch::new(2).unwrap();
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
    intent.owner.runtime_epoch = sage_acp_protocol::contract::RuntimeEpoch::new(2).unwrap();
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
