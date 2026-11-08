//! The restore against what else happens while it runs: a newer pick of the
//! same option, a harness slow to answer, a saved model no longer offered,
//! a pick made before a live runtime reports, and a page near its budget.
use serde_json::json;

use super::restore_tests::{
    choose, effort, mode, option, owner, seam, seed, shown_effort, start, start_reserved, stored,
};
use super::stored_tests::seed_chat;
use super::*;
use crate::{
    acp::{
        runtime::HostError,
        session_config::{
            ConfigId, ConfigMutation, choices,
            notices::{self, NoticeReason, SessionConfigNotice},
        },
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

fn notice(saved: &str, current: &str, reason: NoticeReason) -> SessionConfigNotice {
    SessionConfigNotice {
        config_id: ConfigId::new("configId", "model").unwrap(),
        saved: ConfigId::new("saved", saved).unwrap(),
        current: ConfigId::new("current", current).unwrap(),
        reason,
    }
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
        .restore_until(&resumed, || {
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
fn a_saved_model_falls_back_only_while_no_live_pick_replaced_it() {
    // The stored model is no longer offered; while the mode is restored the
    // user picks model B live. The fallback to the session's model A (read
    // from what the restore began with) must not replace B, nor say A is in
    // use.
    let service = service();
    seed_chat(&service);
    store(&service, &mode("code"), 1);
    store(&service, &option("model", "retired"), 2);
    let resumed = owner(2);
    let agent = start(&service, &resumed, models("a"));
    let mut raced = false;
    service
        .restore_until(&resumed, || {
            if !raced {
                raced = true;
                choose(&service, &resumed, option("model", "b")).unwrap();
            }
            false
        })
        .unwrap();
    assert!(stored(&service).contains(&option("model", "b")));
    assert!(!stored(&service).contains(&option("model", "a")));
    assert_eq!(page_notice(&service), serde_json::Value::Null);
    // Only the mode and the user's pick reached the harness.
    assert_eq!(agent.lock().unwrap().requests.len(), 2);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_pick_confirmed_while_the_restore_waits_for_the_harness_is_kept() {
    // The restore sends the stored "high"; while the harness answers, a
    // confirmed pick of "low" is stored. The restore's write is a
    // compare-and-set against the "high" it read, so "low" stays.
    let service = service();
    seed(&service);
    store(&service, &option("effort", "high"), 1);
    let resumed = owner(2);
    let agent = start(&service, &resumed, effort("low"));
    let database = service.database.clone();
    let session = resumed.session_id.clone();
    agent.lock().unwrap().on_send = Some(Box::new(move || {
        assert!(
            choices::record(
                &database.connection(),
                &session,
                &option("effort", "low"),
                9
            )
            .unwrap()
        );
    }));
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(agent.lock().unwrap().requests.len(), 1);
    assert_eq!(stored(&service), [option("effort", "low")]);
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
    let delay = std::time::Duration::from_millis(60);
    agent.lock().unwrap().delay = delay;
    seam::take_sent();
    service.restore_session_config(&resumed).unwrap();
    // Recorded where each deadline is set, just before its change is sent.
    let sent = seam::take_sent();
    assert_eq!(sent.len(), 2);
    let timeout =
        std::time::Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc);
    for (set_at, deadline) in &sent {
        // The whole per-request timeout from when the change is sent, not
        // what an earlier change left of a shared one.
        assert_eq!(*deadline, *set_at + timeout);
    }
    // The second is set only after the first was answered.
    assert!(sent[1].0 >= sent[0].0 + delay);
    // And it is the deadline the runtime gave the harness.
    let given: Vec<_> = agent
        .lock()
        .unwrap()
        .deadlines
        .iter()
        .map(|d| d.1)
        .collect();
    assert_eq!(given, [sent[0].1, sent[1].1]);
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
        json!({"configId":"model","saved":"retired","current":"a","reason":"notOffered"})
    );
    // Choosing a model settles it.
    choose(&service, &resumed, option("model", "b")).unwrap();
    assert_eq!(page_notice(&service), serde_json::Value::Null);
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_saved_model_the_harness_refuses_is_noted_as_refused() {
    let service = service();
    seed_chat(&service);
    store(&service, &option("model", "b"), 1);
    let resumed = owner(2);
    let agent = start(&service, &resumed, models("a"));
    agent.lock().unwrap().refuse = true;
    service.restore_session_config(&resumed).unwrap();
    assert_eq!(stored(&service), [option("model", "a")]);
    assert_eq!(
        page_notice(&service),
        json!({"configId":"model","saved":"b","current":"a","reason":"rejected"})
    );
    service.shutdown_blocking().unwrap();
}

#[test]
fn a_queued_pick_settles_the_notice_about_its_option() {
    // A parked chat whose saved model was not taken back: picking the model
    // already in use clears the notice in the same write.
    let service = service();
    seed_chat(&service);
    let first = owner(1);
    start(&service, &first, models("a"));
    service.shutdown_blocking().unwrap();
    notices::set(
        &service.database.connection(),
        &first.session_id,
        &notice("retired", "a", NoticeReason::NotOffered),
        1,
    )
    .unwrap();
    assert_eq!(page_notice(&service)["saved"], "retired");
    service
        .queue_session_config_choice(&first.session_id, None, &option("model", "a"))
        .unwrap();
    assert_eq!(page_notice(&service), serde_json::Value::Null);
    assert_eq!(stored(&service), [option("model", "a")]);
}

#[test]
fn a_pick_made_before_a_live_runtime_reports_is_applied_once_it_does() {
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
    // pick is stored, not refused.
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
    // The runtime reports its default; the restore applies the pick once,
    // and a later restore has nothing left to send.
    let agent = start_reserved(&service, &resumed, effort("low"));
    service.restore_session_config(&resumed).unwrap();
    service.restore_session_config(&resumed).unwrap();
    let requests = agent.lock().unwrap().requests.clone();
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0]["params"]["value"], "high");
    assert_eq!(shown_effort(&service, &resumed), "high");
    assert_eq!(stored(&service), [option("effort", "high")]);
    service.shutdown_blocking().unwrap();
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
    notices::set(
        &service.database.connection(),
        &first.session_id,
        &notice("retired", "b", NoticeReason::NotOffered),
        1,
    )
    .unwrap();
    let load = || {
        service
            .load_chat_page(first.project_id.clone(), first.chat_id.clone(), None)
            .unwrap()
    };
    let mut page = load();
    let full = page.restored_session_config.clone().unwrap();
    assert_eq!(
        full.config.config_options.as_slice()[0].name.as_str(),
        "Model"
    );
    let bytes = |page: &AcpChatPage| serde_json::to_vec(page).unwrap().len();
    let stored_only =
        |session: &crate::acp::contract::SageAgentSessionId| service.stored_choices_view(session);
    // A page this close to its budget has no room for the labels: it shows
    // the stored choice on its own, and keeps the notice.
    let near_bound = bytes(&page) - 1;
    fit_config_to_page_budget(&mut page, near_bound, stored_only).unwrap();
    let only = page.restored_session_config.clone().unwrap();
    assert_eq!(only.config.config_options.as_slice().len(), 1);
    assert_eq!(
        only.config.config_options.as_slice()[0].name.as_str(),
        "model"
    );
    assert!(page.session_config_notice.is_some());
    assert!(bytes(&page) <= near_bound);
    // With no room even for that, the transcript loads without it, and
    // then without the notice, before the page is ever refused.
    let mut bare = load();
    bare.restored_session_config = None;
    bare.session_config_notice = None;
    let bare = bytes(&bare);
    let mut page = load();
    fit_config_to_page_budget(&mut page, bare, stored_only).unwrap();
    assert!(page.restored_session_config.is_none());
    assert!(page.session_config_notice.is_none());
    // And a page whose history alone is over budget is still refused.
    assert!(matches!(
        fit_config_to_page_budget(&mut page, bare - 1, |_| None),
        Err(StoreError::Capacity { .. })
    ));
}
