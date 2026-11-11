use super::*;
use crate::acp::{goal::NativeRpcReply, session_control::SessionControl, usage::UsageState};

fn stats() -> Value {
    json!({
        "sessionFile":"/sessions/1_pi.jsonl", "sessionId":"pi-session-1",
        "userMessages":2, "assistantMessages":2, "toolCalls":1, "toolResults":1, "totalMessages":6,
        "tokens":{"input":1200,"output":340,"cacheRead":800,"cacheWrite":60,"total":2400},
        "cost":0.0125,
        "contextUsage":{"tokens":2000,"contextWindow":200000,"percent":1}
    })
}

fn spend(runtime: &PiRuntime) -> Option<crate::acp::usage::SessionSpend> {
    match runtime.usage().unwrap().state() {
        UsageState::Usage { snapshot } => snapshot.session,
        _ => None,
    }
}

#[test]
fn a_usage_read_asks_pi_for_session_stats_and_keeps_its_totals_and_cost() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    assert_eq!(runtime.usage().unwrap().state(), UsageState::Unavailable);
    let read = runtime.read_usage().unwrap();
    let query = deliver(&mut runtime, read);
    assert_eq!(query, json!({"type":"get_session_stats","id":query["id"]}));
    assert!(runtime.native_rpc_pending());
    // One native request at a time.
    assert!(runtime.read_usage().is_err());
    let batch = runtime.ingest(&response(&query, stats()), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
    assert!(!runtime.native_rpc_pending());
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    let spend = spend(&runtime).unwrap();
    assert_eq!(
        (spend.input_tokens, spend.output_tokens, spend.total_tokens),
        (1200, 340, 2400)
    );
    assert_eq!(
        (
            spend.cache_read_tokens,
            spend.cache_write_tokens,
            spend.cost_usd
        ),
        (800, 60, Some(0.0125))
    );
}

#[test]
fn a_usage_read_works_while_a_turn_runs_and_leaves_the_turn_alone() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let read = runtime.read_usage().unwrap();
    let query = deliver(&mut runtime, read);
    let batch = runtime.ingest(&response(&query, stats()), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    // The turn still streams its answer.
    runtime
        .ingest(&frame(json!({"type":"message_start"})), &Mapper)
        .unwrap();
    let delta = runtime
        .ingest(
            &frame(json!({"type":"message_update","assistantMessageEvent":
                {"type":"text_delta","delta":"still here"}})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        delta.events(),
        [AdapterEvent::Item {
            payload: NormalizedPayload::Text { .. },
            ..
        }]
    ));
}

#[test]
fn a_refused_or_unreadable_stats_reply_is_rejected_without_a_transcript_error() {
    for reply in [
        json!({"type":"response","command":"get_session_stats","success":false,"error":"No session"}),
        json!({"type":"response","command":"get_session_stats","success":true,"data":{"tokens":{}}}),
    ] {
        let mut runtime = fixture_runtime(1);
        ready(&mut runtime);
        let read = runtime.read_usage().unwrap();
        let query = deliver(&mut runtime, read);
        let mut reply = reply;
        reply["id"] = query["id"].clone();
        let batch = runtime.ingest(&frame(reply), &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
        assert_eq!(
            runtime.take_native_rpc_reply(),
            Some(NativeRpcReply::Rejected)
        );
        assert_eq!(runtime.usage().unwrap().state(), UsageState::Unavailable);
    }
}

#[test]
fn stats_that_arrive_after_the_caller_gave_up_refresh_usage_but_answer_no_one() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let read = runtime.read_usage().unwrap();
    let query = deliver(&mut runtime, read);
    runtime.abandon_usage_read();
    assert!(!runtime.native_rpc_pending());
    let batch = runtime.ingest(&response(&query, stats()), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert_eq!(spend(&runtime).unwrap().total_tokens, 2400);
}

#[test]
fn a_reply_to_an_abandoned_read_never_answers_the_next_caller() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let first = runtime.read_usage().unwrap();
    let first = deliver(&mut runtime, first);
    runtime.abandon_usage_read();
    let control = SessionControl::SetTitle {
        title: RequiredText::new("sessionTitle", "Named").unwrap(),
    };
    let rename = runtime.session_control(&control).unwrap();
    let rename = deliver(&mut runtime, rename);
    // The old stats reply arrives while the rename waits.
    let late = runtime.ingest(&response(&first, stats()), &Mapper).unwrap();
    assert!(matches!(late.events(), [AdapterEvent::IgnoredLate]));
    assert!(runtime.native_rpc_pending());
    assert_eq!(runtime.take_native_rpc_reply(), None);
    runtime
        .ingest(&response(&rename, Value::Null), &Mapper)
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
}

#[test]
fn a_usage_read_that_was_never_written_frees_the_native_slot() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let read = runtime.read_usage().unwrap();
    runtime
        .resolve_egress(read.into_token(), EgressResolution::QueueRejected)
        .unwrap();
    assert!(!runtime.native_rpc_pending());
    let read = runtime.read_usage().unwrap();
    assert_eq!(deliver(&mut runtime, read)["type"], "get_session_stats");
}

#[test]
fn renaming_the_chat_names_pis_session() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let control = SessionControl::SetTitle {
        title: RequiredText::new("sessionTitle", "Fix the login flow").unwrap(),
    };
    let egress = runtime.session_control(&control).unwrap();
    let request = deliver(&mut runtime, egress);
    assert_eq!(
        request,
        json!({"type":"set_session_name","id":request["id"],"name":"Fix the login flow"})
    );
    let batch = runtime
        .ingest(&response(&request, Value::Null), &Mapper)
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Absorbed]));
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Controlled(None))
    );
    // Pi's own echo of the name is the chat title it already has.
    let echo = runtime
        .ingest(
            &frame(json!({"type":"session_info_changed","name":"Fix the login flow"})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(echo.events(), [AdapterEvent::Session { .. }]));
}

#[test]
fn controls_pi_has_no_method_for_are_refused_before_anything_is_sent() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let stop = SessionControl::StopActivity {
        activity_id: ExternalId::new("activity", "task-1").unwrap(),
    };
    assert!(matches!(
        runtime.session_control(&stop),
        Err(AdapterRuntimeError::UnsupportedCommand)
    ));
    assert!(!runtime.native_rpc_pending());
}

#[test]
fn native_requests_wait_for_pis_session() {
    let mut runtime = fixture_runtime(1);
    initialize(&mut runtime);
    assert!(runtime.read_usage().is_err());
    let control = SessionControl::SetTitle {
        title: RequiredText::new("sessionTitle", "Named").unwrap(),
    };
    assert!(runtime.session_control(&control).is_err());
}
