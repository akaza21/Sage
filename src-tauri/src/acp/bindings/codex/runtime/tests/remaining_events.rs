use super::*;
use crate::acp::intent::session_event::{SessionNotice, SessionNoticeLevel, SessionScopedEvent};

mod approval_reviews;

fn notify(runtime: &mut CodexRuntime, method: &str, params: Value) -> Vec<AdapterEvent> {
    runtime
        .ingest(&frame(json!({"method":method,"params":params})), &Mapper)
        .expect("notification")
        .events()
        .to_vec()
}

fn rejected(runtime: &mut CodexRuntime, method: &str, params: Value) -> AdapterRuntimeError {
    match runtime.ingest(&frame(json!({"method":method,"params":params})), &Mapper) {
        Ok(batch) => panic!("expected a rejection, got {:?}", batch.events()),
        Err(error) => error,
    }
}

fn one_notice(events: &[AdapterEvent]) -> SessionNotice {
    match events {
        [
            AdapterEvent::Session {
                id,
                event: SessionScopedEvent::Notice(notice),
            },
        ] => {
            assert_eq!(id.external_id.as_str(), "thread-1");
            notice.clone()
        }
        other => panic!("expected one notice, got {other:?}"),
    }
}

fn running() -> CodexRuntime {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    running_turn(&mut runtime);
    runtime
}

fn compaction_items(events: &[AdapterEvent]) -> usize {
    events
        .iter()
        .filter(|event| {
            matches!(
                event,
                AdapterEvent::Item {
                    lifecycle: ItemLifecycle::Completed,
                    payload: NormalizedPayload::CompactionNotice,
                    ..
                }
            )
        })
        .count()
}

fn compaction_item(runtime: &mut CodexRuntime, id: &str) -> Vec<AdapterEvent> {
    notify(
        runtime,
        "item/completed",
        json!({"threadId":"thread-1","turnId":"turn-1",
            "item":{"type":"contextCompaction","id":id}}),
    )
}

fn legacy_compaction(runtime: &mut CodexRuntime) -> Vec<AdapterEvent> {
    notify(
        runtime,
        "thread/compacted",
        json!({"threadId":"thread-1","turnId":"turn-1"}),
    )
}

#[test]
fn auth_recovery_start_and_completion_are_session_notices() {
    let mut runtime = running();
    for (method, text) in [
        (
            "modelProvider/authRecoveryStarted",
            "Codex is renewing its sign-in to openai: Refreshing the access token.",
        ),
        (
            "modelProvider/authRecoveryCompleted",
            "Codex finished renewing its sign-in to openai: Refreshing the access token.",
        ),
    ] {
        let notice = one_notice(&notify(
            &mut runtime,
            method,
            json!({"threadId":"thread-1","turnId":"turn-1","provider":"openai",
                "message":"Refreshing the access token.\u{202e}"}),
        ));
        assert_eq!(notice.level, SessionNoticeLevel::Info);
        assert_eq!(notice.text.as_str(), text);
        assert_eq!(notice.source.unwrap().as_str(), "codex sign-in");
    }
    let notice = one_notice(&notify(
        &mut runtime,
        "modelProvider/authRecoveryCompleted",
        json!({"threadId":"thread-1","turnId":"turn-1","provider":"openai","message":""}),
    ));
    assert_eq!(
        notice.text.as_str(),
        "Codex finished renewing its sign-in to openai."
    );
    assert!(matches!(
        notify(
            &mut runtime,
            "modelProvider/authRecoveryStarted",
            json!({"threadId":"thread-1","provider":7}),
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn moderation_metadata_is_identity_checked_and_not_projected() {
    let mut runtime = running();
    let params = json!({"threadId":"thread-1","turnId":"turn-1","metadata":{"flagged":false}});
    assert!(matches!(
        notify(&mut runtime, "turn/moderationMetadata", params.clone()).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    let mut foreign = params;
    foreign["threadId"] = json!("thread-foreign");
    assert!(matches!(
        rejected(&mut runtime, "turn/moderationMetadata", foreign),
        AdapterRuntimeError::Operation
    ));
}

#[test]
fn world_writable_warning_names_a_few_folders_and_counts_the_rest() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let notice = one_notice(&notify(
        &mut runtime,
        "windows/worldWritableWarning",
        json!({"samplePaths":[r"C:\repo\cache",r"C:\repo\logs",r"C:\repo\out",r"C:\repo\tmp"],
            "extraCount":2,"failedScan":true}),
    ));
    assert_eq!(notice.level, SessionNoticeLevel::Warning);
    assert_eq!(
        notice.text.as_str(),
        "Codex's Windows sandbox cannot protect folders that other users can write to. \
         It found 6, including cache, logs, out.\n\
         Codex could not finish checking for them."
    );
    assert!(matches!(
        rejected(
            &mut runtime,
            "windows/worldWritableWarning",
            json!({"samplePaths":[],"extraCount":-1,"failedScan":false}),
        ),
        AdapterRuntimeError::Frame
    ));
}

#[test]
fn an_auto_approval_review_leaves_one_notice_with_its_verdict() {
    let mut runtime = running();
    let review = |id: &str, status: &str| {
        json!({"threadId":"thread-1","turnId":"turn-1","reviewId":id,
            "targetItemId":"item-1","startedAtMs":1,
            "action":{"type":"command","command":"cargo test","cwd":r"C:\repo","source":"shell"},
            "review":{"status":status,"riskLevel":"low","rationale":"Runs the test suite."}})
    };
    assert!(matches!(
        notify(
            &mut runtime,
            "item/autoApprovalReview/started",
            review("review-1", "inProgress")
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    let notice = one_notice(&notify(
        &mut runtime,
        "item/autoApprovalReview/completed",
        review("review-1", "denied"),
    ));
    assert_eq!(notice.level, SessionNoticeLevel::Warning);
    assert_eq!(
        notice.text.as_str(),
        "Codex's approval reviewer denied a command (risk: low). Runs the test suite."
    );
    assert_eq!(notice.source.unwrap().as_str(), "codex approval reviewer");
    // Each filtered status has a fresh identity so duplicate suppression
    // cannot hide an incorrectly emitted notice.
    let mut unknown = review("review-unknown", "someFutureStatus");
    unknown["action"] = json!({"type":"someFutureAction"});
    let mut missing = review("review-missing", "denied");
    missing.as_object_mut().unwrap().remove("action");
    missing["review"] = json!(7);
    for params in [
        review("review-approved", "approved"),
        review("review-aborted", "aborted"),
        unknown,
        missing,
    ] {
        assert!(matches!(
            notify(&mut runtime, "item/autoApprovalReview/completed", params).as_slice(),
            [AdapterEvent::IgnoredLate]
        ));
    }
    // An unknown action still names its verdict.
    let mut timed_out = review("review-timed-out", "timedOut");
    timed_out["action"] = json!({"type":"someFutureAction"});
    timed_out["review"]["rationale"] = Value::Null;
    assert_eq!(
        one_notice(&notify(
            &mut runtime,
            "item/autoApprovalReview/completed",
            timed_out
        ))
        .text
        .as_str(),
        "Codex's approval reviewer timed out reviewing a request (risk: low)."
    );
}

#[test]
fn legacy_compaction_alone_shows_the_compaction_row() {
    let mut runtime = running();
    let events = legacy_compaction(&mut runtime);
    assert_eq!(compaction_items(&events), 1, "{events:?}");
    // A second compaction in the same Turn is a row of its own.
    assert_eq!(compaction_items(&legacy_compaction(&mut runtime)), 1);
}

#[test]
fn legacy_and_item_compaction_show_one_row_in_either_order() {
    let mut runtime = running();
    assert_eq!(compaction_items(&compaction_item(&mut runtime, "c-1")), 1);
    assert!(matches!(
        legacy_compaction(&mut runtime).as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    assert_eq!(compaction_items(&legacy_compaction(&mut runtime)), 1);
    assert!(matches!(
        compaction_item(&mut runtime, "c-2").as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}

#[test]
fn thread_settings_update_refreshes_the_model_and_effort() {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("gpt-6-astra"), Some("high"));
    let settings = |model: &str, effort: Value| {
        json!({"threadId":"thread-1","threadSettings":{
            "model":model,"effort":effort,"modelProvider":"openai","cwd":r"C:\repo",
            "approvalPolicy":"on-request","approvalsReviewer":"user",
            "sandboxPolicy":{"type":"readOnly"},
            "collaborationMode":{"mode":"default","settings":{}}
        }})
    };
    // Settings during activation do not request the catalog; the idle owner
    // starts hydration after handoff.
    let update =
        json!({"method":"thread/settings/updated","params":settings("gpt-6-astra",json!("high"))});
    let batch = runtime.ingest(&frame(update), &Mapper).unwrap();
    assert!(batch.egress().is_empty());
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{"data":[{
                "id":"gpt-6-astra","displayName":"GPT-6 Astra","isDefault":true,
                "supportedReasoningEfforts":[
                    {"reasoningEffort":"low","description":"Quick"},
                    {"reasoningEffort":"medium","description":"Balanced"},
                    {"reasoningEffort":"high","description":"Deep"}
                ]
            },{
                "id":"gpt-6-nova","displayName":"GPT-6 Nova","isDefault":false,
                "supportedReasoningEfforts":[{"reasoningEffort":"low","description":"Quick"}]
            }],"nextCursor":null}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        notify(
            &mut runtime,
            "thread/settings/updated",
            settings("gpt-6-astra", json!("medium"))
        )
        .as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    let config = runtime.session_config().unwrap();
    assert_eq!(
        serde_json::to_value(&config).unwrap()["configOptions"][0]["currentValue"],
        "medium"
    );
    // A null effort keeps the selected one for the same model.
    notify(
        &mut runtime,
        "thread/settings/updated",
        settings("gpt-6-astra", Value::Null),
    );
    let config = runtime.session_config().unwrap();
    assert_eq!(config.confirmed_effort.unwrap().as_str(), "medium");
    // A new model takes its efforts from the catalog.
    notify(
        &mut runtime,
        "thread/settings/updated",
        settings("gpt-6-nova", json!("low")),
    );
    let config = serde_json::to_value(runtime.session_config().unwrap()).unwrap();
    assert_eq!(config["confirmedModel"]["name"], "GPT-6 Nova");
    assert_eq!(config["configOptions"][0]["currentValue"], "low");
    assert_eq!(config["configOptions"][0]["options"][0]["value"], "low");
    assert_eq!(config["configOptions"][0]["options"][1], Value::Null);
    // A new model without an effort drops the old model's effort, even if
    // both offer it, so the next turn/start sends none.
    notify(
        &mut runtime,
        "thread/settings/updated",
        settings("gpt-6-astra", Value::Null),
    );
    let config = runtime.session_config().unwrap();
    assert_eq!(config.confirmed_model.unwrap().id.as_str(), "gpt-6-astra");
    assert!(config.confirmed_effort.is_none());
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "continue").unwrap(),
            client_message_id: RequiredText::new("client", "settings-effort").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let request: Value = serde_json::from_slice(prompt.frame().payload()).unwrap();
    assert!(request["params"].get("effort").is_none());
    let mut foreign = settings("gpt-6-astra", Value::Null);
    foreign["threadId"] = json!("thread-foreign");
    assert!(matches!(
        rejected(&mut runtime, "thread/settings/updated", foreign),
        AdapterRuntimeError::Operation
    ));
}
