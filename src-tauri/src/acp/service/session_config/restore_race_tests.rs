//! The restore against what else happens while it runs: a newer pick of the
//! same option, a harness slow to answer, a saved model no longer offered,
//! a pick made before a live runtime reports, and a page near its budget.
use serde_json::json;

use super::restore_tests::{
    choose, effort, mode, option, owner, seed, shown_effort, start, stored,
};
use super::stored_tests::seed_chat;
use super::*;
use crate::{
    acp::{
        runtime::HostError,
        session_config::{ConfigMutation, choices},
    },
    persistence::Database,
};

fn service() -> AcpService {
    AcpService::new(Database::open(std::path::Path::new(":memory:")).unwrap()).unwrap()
}

fn store(service: &AcpService, choice: &ConfigMutation, at: i64) {
    assert!(
        choices::record(
            &service.database.connection(),
            &owner(1).session_id,
            choice,
            at
        )
        .unwrap()
    );
}

fn far() -> Option<std::time::Instant> {
    Some(std::time::Instant::now() + std::time::Duration::from_secs(60))
}

#[test]
fn a_pick_of_the_same_option_made_during_the_restore_is_not_overwritten() {
    let service = service();
    seed(&service);
    store(&service, &option("effort", "high"), 1);
    let resumed = owner(2);
    let agent = start(
        &service,
        &resumed,
        json!([{
            "id":"effort","name":"Effort","category":"thought_level","type":"select",
            "currentValue":"low",
            "options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]
        }]),
    );
    // Just before the stored "high" is sent, the user picks "medium" live.
    // The restore's change then meets a newer revision: it re-reads the
    // stored choice, finds the user's, and neither resends nor stores the
    // older value.
    let mut raced = false;
    service
        .restore_until(&resumed, far(), || {
            if !raced {
                raced = true;
                choose(&service, &resumed, option("effort", "medium")).unwrap();
            }
            false
        })
        .unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["params"]["value"], "medium");
    assert_eq!(shown_effort(&service, &resumed), "medium");
    assert_eq!(stored(&service), [option("effort", "medium")]);
    // Nothing is left for the next prompt to put back.
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    assert_eq!(shown_effort(&service, &resumed), "medium");
    service.shutdown_blocking().unwrap();
}

#[test]
fn each_restore_change_gets_its_own_deadline_and_a_slow_answer_is_not_fatal() {
    let service = service();
    seed(&service);
    store(&service, &mode("code"), 1);
    store(&service, &option("effort", "high"), 2);
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    agent.lock().unwrap().delay = std::time::Duration::from_millis(60);
    service.restore_session_config(&resumed).unwrap();
    let deadlines = agent.lock().unwrap().deadlines.clone();
    assert_eq!(deadlines.len(), 2);
    let timeout =
        std::time::Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc);
    for (sent, deadline) in &deadlines {
        // Each change has the whole per-request timeout from when it is
        // sent, not what an earlier change left of a shared one.
        assert!(deadline.duration_since(*sent) + std::time::Duration::from_millis(50) >= timeout);
    }
    assert!(deadlines[1].1 >= deadlines[0].1 + std::time::Duration::from_millis(50));
    assert_eq!(shown_effort(&service, &resumed), "high");
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_restore_change_not_sent_in_time_stays_pending_and_the_session_keeps_running() {
    // Nothing reached the harness: no recovery, the choice stays stored, and
    // it is sent before the next prompt.
    let service = service();
    seed(&service);
    store(&service, &option("effort", "high"), 1);
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    agent.lock().unwrap().fail = Some(HostError::NotDispatched);
    service.restore_session_config(&resumed).unwrap();
    let host = service.lane_at(0).runtime.for_owner(&resumed).unwrap();
    assert!(!matches!(
        host.state(),
        Ok(crate::acp::runtime::HostState::Recovery) | Err(_)
    ));
    assert!(stored(&service).contains(&option("effort", "high")));
    assert_eq!(shown_effort(&service, &resumed), "low");
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(shown_effort(&service, &resumed), "high");
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_restore_change_left_unanswered_keeps_its_choice_for_the_resumed_runtime() {
    // The harness may still apply it, so the runtime is recovered rather
    // than trusted; the stored choice is neither replaced nor dropped, and
    // the next runtime of the session restores it.
    let service = service();
    seed(&service);
    store(&service, &option("effort", "high"), 1);
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    agent.lock().unwrap().fail = Some(HostError::Deadline);
    assert_eq!(
        service.restore_session_config(&resumed),
        Err(HostError::Deadline)
    );
    assert_eq!(stored(&service), [option("effort", "high")]);
    service.shutdown_blocking().unwrap();
}

fn models(current: &str) -> serde_json::Value {
    json!([{
        "id":"model","name":"Model","category":"model","type":"select","currentValue":current,
        "options":[{"value":"a","name":"Model A"},{"value":"b","name":"Model B"}]
    }])
}

fn page_notice(service: &AcpService) -> serde_json::Value {
    let first = owner(1);
    let page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    serde_json::to_value(&page).unwrap()["sessionConfigNotice"].clone()
}

#[test]
fn a_saved_model_no_longer_offered_is_noted_for_the_composer() {
    let service = service();
    seed_chat(&service);
    store(&service, &option("model", "retired"), 1);
    let resumed = owner(2);
    start(&service, &resumed, models("a"));
    service.restore_session_config(&resumed).unwrap();
    // The session keeps its own model, stored as the chat's, and says so.
    assert_eq!(stored(&service), [option("model", "a")]);
    assert_eq!(
        page_notice(&service),
        "The saved model retired isn't offered any more; this chat uses Model A."
    );
    // Choosing a model settles it.
    choose(&service, &resumed, option("model", "b")).unwrap();
    assert_eq!(page_notice(&service), serde_json::Value::Null);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_pick_made_before_a_live_runtime_reports_is_queued_not_dropped() {
    let directory =
        std::env::temp_dir().join(format!("sage-config-unreported-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("sage.db");
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    seed_chat(&service);
    let first = owner(1);
    start(&service, &first, effort("low"));
    // A live runtime that reports its config takes changes through it.
    assert_eq!(
        service.queue_session_config_choice(
            &first.session_id,
            Some(&first),
            &option("effort", "high")
        ),
        Err(super::QueueError::Live)
    );
    service.shutdown_blocking().unwrap();
    drop(service);
    // After a restart, a runtime that has started but not reported yet: the
    // pick is stored, not refused, and the restore applies it once the
    // session reports.
    let service = AcpService::new(Database::open(&path).unwrap()).unwrap();
    let resumed = owner(2);
    service
        .lane_at(0)
        .runtime
        .reserve_start(resumed.clone())
        .unwrap();
    service
        .queue_session_config_choice(
            &resumed.session_id,
            Some(&resumed),
            &option("effort", "high"),
        )
        .unwrap();
    assert_eq!(stored(&service), [option("effort", "high")]);
    drop(service);
    std::fs::remove_dir_all(directory).unwrap();
}

#[test]
fn a_restored_config_that_does_not_fit_the_page_shows_only_the_stored_choices() {
    let service = service();
    seed_chat(&service);
    let first = owner(1);
    start(&service, &first, models("a"));
    choose(&service, &first, option("model", "b")).unwrap();
    service.shutdown_blocking().unwrap();
    let mut page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    let full = page.restored_session_config.clone().unwrap();
    assert_eq!(
        full.config.config_options.as_slice()[0].name.as_str(),
        "Model"
    );
    let bytes = |page: &AcpChatPage| serde_json::to_vec(page).unwrap().len();
    // A page this close to its budget has no room for the labels: it shows
    // the stored choice on its own.
    let near_bound = bytes(&page) - 1;
    fit_restored_to_page_budget(&mut page, near_bound, |session| {
        service.stored_choices_view(session)
    })
    .unwrap();
    let only = page.restored_session_config.clone().unwrap();
    assert_eq!(only.config.config_options.as_slice().len(), 1);
    assert_eq!(
        only.config.config_options.as_slice()[0].name.as_str(),
        "model"
    );
    assert!(bytes(&page) <= near_bound);
    // With no room even for that, the transcript still loads without it.
    let mut page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    page.restored_session_config = None;
    let bare = bytes(&page);
    let mut page = service
        .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
        .unwrap();
    fit_restored_to_page_budget(&mut page, bare, |session| {
        service.stored_choices_view(session)
    })
    .unwrap();
    assert!(page.restored_session_config.is_none());
    // And a page whose history alone is over budget is still refused.
    assert!(matches!(
        fit_restored_to_page_budget(&mut page, bare - 1, |_| None),
        Err(StoreError::Capacity { .. })
    ));
}
