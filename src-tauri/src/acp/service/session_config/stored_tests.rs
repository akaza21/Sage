use serde_json::{Value, json};

use super::restore_tests::{choose, effort, option, owner, seed, start, stored};

pub(super) fn seed_chat(service: &AcpService) {
    seed(service);
    service
        .database
        .connection()
        .execute(
            "INSERT INTO chat_session_heads VALUES('project-restore','chat-restore','session-restore',0,1)",
            [],
        )
        .unwrap();
}
use super::*;
use crate::{
    acp::session_config::{ConfigId, ConfigMutation, ConfigValue},
    persistence::Database,
};

fn restored_page(service: &AcpService) -> Value {
    let first = owner(1);
    let page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    assert!(page.session_config.is_none());
    serde_json::to_value(&page).unwrap()["restoredSessionConfig"].clone()
}

fn reopen(path: &std::path::Path, service: AcpService) -> AcpService {
    service.shutdown_blocking().unwrap();
    drop(service);
    AcpService::new(Database::open(path).unwrap()).unwrap()
}

#[test]
fn a_parked_chat_shows_the_stored_choice_with_its_labels_before_resuming() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-stored-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    seed_chat(&service);
    assert_eq!(restored_page(&service), Value::Null);
    let first = owner(1);
    start(&service, &first, effort("low"));
    choose(&service, &first, option("effort", "high")).unwrap();
    let service = reopen(&path, service);
    let restored = restored_page(&service);
    assert_eq!(restored["sageAgentSessionId"], "session-restore");
    assert_eq!(restored["awaiting"], "resume");
    assert_eq!(
        restored["configOptions"],
        json!([{
            "id":"effort","name":"Effort","category":"thought_level","type":"select",
            "currentValue":"high",
            "options":[{"value":"low","name":"Low"},{"value":"high","name":"High"}]
        }])
    );
    assert!(restored.get("runtimeEpoch").is_none());
    service.shutdown_blocking().unwrap();
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_choice_made_while_parked_shows_at_once_and_is_restored_on_resume() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-queued-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    seed_chat(&service);
    start(&service, &owner(1), effort("low"));
    let service = reopen(&path, service);
    let session = owner(1).session_id;
    assert_eq!(
        restored_page(&service)["configOptions"][0]["currentValue"],
        "low"
    );
    assert_eq!(
        service.queue_session_config_choice(&session, None, &option("effort", "max")),
        Err(QueueError::NotOffered)
    );
    assert_eq!(
        service.queue_session_config_choice(
            &session,
            None,
            &ConfigMutation::Mode(ConfigId::new("modeId", "plan").unwrap())
        ),
        Err(QueueError::NotOffered)
    );
    service
        .queue_session_config_choice(&session, None, &option("effort", "high"))
        .unwrap();
    assert_eq!(stored(&service), [option("effort", "high")]);
    assert_eq!(
        restored_page(&service)["configOptions"][0]["currentValue"],
        "high"
    );
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    service.restore_session_config(&resumed).unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(
        requests[0]["params"],
        json!({"sessionId":"external-a","configId":"effort","value":"high"})
    );
    service.shutdown_blocking().unwrap();
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_stored_choice_without_a_snapshot_still_shows_its_value() {
    let service =
        AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap();
    seed_chat(&service);
    let choice = ConfigMutation::Option {
        config_id: ConfigId::new("configId", "model").unwrap(),
        value: ConfigValue::Select {
            value: ConfigId::new("value", "fixture-model-deep").unwrap(),
        },
    };
    crate::acp::session_config::choices::record(
        &service.database.connection(),
        &owner(1).session_id,
        &choice,
        1,
    )
    .unwrap();
    assert_eq!(
        restored_page(&service)["configOptions"],
        json!([{
            "id":"model","name":"model","type":"select","currentValue":"fixture-model-deep",
            "options":[{"value":"fixture-model-deep","name":"fixture-model-deep"}]
        }])
    );
}
