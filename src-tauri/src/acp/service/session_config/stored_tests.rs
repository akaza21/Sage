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
use sage_acp_protocol::session_config::{ConfigId, ConfigMutation, ConfigValue};

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
    AcpService::new(crate::database::open_database(path).unwrap()).unwrap()
}

#[test]
fn a_parked_chat_shows_the_stored_choice_with_its_labels_before_resuming() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-stored-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let service = AcpService::new(crate::database::open_database(&path).unwrap()).unwrap();
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
    let service = AcpService::new(crate::database::open_database(&path).unwrap()).unwrap();
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
        AcpService::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap())
            .unwrap();
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

/// A chat whose only session failed after its effort was stored as high.
fn failed_chat() -> AcpService {
    let service =
        AcpService::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap())
            .unwrap();
    seed(&service);
    crate::acp::session_config::choices::record(
        &service.database.connection(),
        &owner(1).session_id,
        &option("effort", "high"),
        1,
    )
    .unwrap();
    fail_session(&service);
    service
}

/// The chat's only session fails before its runtime reported anything, as a
/// failed activation leaves it: no head.
fn fail_session(service: &AcpService) {
    service
        .database
        .connection()
        .execute_batch(
            "UPDATE agent_sessions SET state='failed',terminal_at=1 WHERE id='session-restore';
             DELETE FROM chat_session_heads WHERE session_id='session-restore';
             INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,
               item_count,fact_count,payload_bytes,revision)
             VALUES('project-restore',1,1,0,0,0,0,0);",
        )
        .unwrap();
}

/// Starts the chat's next session, as a send does.
fn start_next_session(
    service: &AcpService,
    binding: &str,
    model: Option<&str>,
) -> sage_acp_protocol::contract::SageAgentSessionId {
    let revision: i64 = service
        .database
        .connection()
        .query_row(
            "SELECT projection_revision FROM chats WHERE id='chat-restore'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    crate::acp::store::start_session(
        &mut service.database.write_connection().unwrap(),
        crate::acp::store::StartSessionCommand {
            project_id: owner(1).project_id,
            chat_id: owner(1).chat_id,
            expected_projection_revision: sage_acp_protocol::contract::ProjectionRevision::new(
                revision as u64,
            )
            .unwrap(),
            client_intent_id: sage_acp_protocol::contract::ClientIntentId::parse("intent-next")
                .unwrap(),
            binding_id: sage_acp_protocol::intent::BindingId::new("bindingId", binding).unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: model
                .map(|model| sage_acp_protocol::intent::ModelId::new("modelId", model).unwrap()),
            now_millis: 2,
        },
    )
    .unwrap()
    .session_id
}

/// Starts the chat's next session and returns the choices it holds.
fn start_next(service: &AcpService, binding: &str, model: Option<&str>) -> Vec<ConfigMutation> {
    let next = start_next_session(service, binding, model);
    crate::acp::session_config::choices::load(&service.database.connection(), &next).unwrap()
}

#[test]
fn a_failed_session_hands_its_choices_to_the_session_the_next_send_starts() {
    let service = failed_chat();
    // No head: the page offers the failed session's choice for the next send.
    let restored = restored_page(&service);
    assert_eq!(restored["sageAgentSessionId"], "session-restore");
    assert_eq!(restored["awaiting"], "start");
    assert_eq!(restored["configOptions"][0]["currentValue"], "high");
    // The session a send starts on the same binding and model takes it over.
    assert_eq!(
        start_next(&service, "opencode-acp", None),
        [option("effort", "high")]
    );
}

#[test]
fn a_session_started_on_another_model_or_binding_takes_no_choices_over() {
    assert!(start_next(&failed_chat(), "opencode-acp", Some("other-model")).is_empty());
    assert!(start_next(&failed_chat(), "claude-sdk", None).is_empty());
}

#[test]
fn the_session_started_after_a_failure_goes_on_from_it_on_the_same_binding_only() {
    let preceding = |service: &AcpService, binding| {
        let next = start_next_session(service, binding, Some("other-model"));
        crate::acp::store::preceding_session(&service.database.connection(), &next)
            .unwrap()
            .map(|session| session.as_str().to_owned())
    };
    assert_eq!(
        preceding(&failed_chat(), "opencode-acp").as_deref(),
        Some("session-restore")
    );
    assert_eq!(preceding(&failed_chat(), "claude-sdk"), None);
}

#[test]
fn a_start_that_failed_before_its_session_reported_still_offers_every_listed_level() {
    let service =
        AcpService::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap())
            .unwrap();
    seed(&service);
    // The send that started the session carried a drafted High, which the
    // binding listed for the model beside Low.
    let model = sage_domain::harness::ModelOption {
        id: "fixture-model".into(),
        name: "Fixture".into(),
        description: None,
        is_default: Some(true),
        context_window: None,
        effort_levels: Some(vec!["low".into(), "high".into()]),
    };
    let offered = crate::acp::bindings::codex::launch_effort_option(&model, "high").unwrap();
    assert!(service.remember_start_choice(
        &owner(1).session_id,
        &option("effort", "high"),
        &offered
    ));
    fail_session(&service);
    let restored = restored_page(&service);
    assert_eq!(restored["awaiting"], "start");
    let effort = &restored["configOptions"][0];
    assert_eq!(effort["currentValue"], "high");
    assert_eq!(
        effort["options"],
        json!([{"value":"low","name":"Low"},{"value":"high","name":"High"}])
    );
    // Low is chosen after the failure, and the session the next send starts
    // takes Low, not the High it was drafted with.
    service
        .queue_session_config_choice(&owner(1).session_id, None, &option("effort", "low"))
        .unwrap();
    assert_eq!(
        restored_page(&service)["configOptions"][0]["currentValue"],
        "low"
    );
    assert_eq!(
        start_next(&service, "opencode-acp", None),
        [option("effort", "low")]
    );
}
