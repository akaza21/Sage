use super::*;
use crate::acp::{goal::NativeRpcReply, usage::UsageState};

fn update(limit: &str, used: i64) -> Frame {
    frame(
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{
            "limitId":limit,"limitName":null,"planType":"plus",
            "primary":{"usedPercent":used,"windowDurationMins":300,"resetsAt":1_762_900_000},
            "secondary":null,"credits":null,"rateLimitReachedType":null
        }}}),
    )
}

fn used(runtime: &CodexRuntime) -> Vec<u8> {
    let UsageState::Usage { snapshot } = runtime.usage().unwrap().state() else {
        panic!("no usage reported");
    };
    snapshot.windows.iter().map(|w| w.used_percent).collect()
}

fn answer(runtime: &mut CodexRuntime, id: u64, body: Value) {
    let mut reply = json!({"id":id});
    reply
        .as_object_mut()
        .unwrap()
        .extend(body.as_object().unwrap().clone());
    let batch = runtime.ingest(&frame(reply), &Mapper).unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn pushed_buckets_merge_by_limit_without_transcript_events() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    assert_eq!(runtime.usage().unwrap().state(), UsageState::Unavailable);
    for frame in [
        update("codex", 12),
        update("spark", 40),
        update("codex", 57),
    ] {
        let batch = runtime.ingest(&frame, &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    }
    assert_eq!(used(&runtime), vec![57, 40]);
    let bogus = frame(json!({"method":"account/rateLimits/updated","params":{"rateLimits":{}}}));
    runtime.ingest(&bogus, &Mapper).unwrap();
    assert_eq!(used(&runtime), vec![57, 40]);
}

#[test]
fn usage_read_answers_the_native_rpc_and_a_refusal_marks_usage_unsupported() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    runtime.ingest(&update("codex", 30), &Mapper).unwrap();

    let read = runtime.read_usage().unwrap();
    let wire: Value = serde_json::from_slice(read.frame().payload()).unwrap();
    assert_eq!(wire["method"], "account/rateLimits/read");
    assert!(runtime.native_rpc_pending());
    assert!(runtime.read_usage().is_err());
    answer(
        &mut runtime,
        wire["id"].as_u64().unwrap(),
        json!({"result":{
            "rateLimits":{"limitId":"codex","primary":{"usedPercent":64}},
            "rateLimitsByLimitId":null,"ordinaryUsageAllowed":true
        }}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    assert!(!runtime.native_rpc_pending());
    assert_eq!(used(&runtime), vec![64]);

    let read = runtime.read_usage().unwrap();
    answer(
        &mut runtime,
        request_id(&read),
        json!({"error":{"code":-32600,"message":"chatgpt authentication required"}}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    assert_eq!(runtime.usage().unwrap().state(), UsageState::Unsupported);
}

#[test]
fn a_backend_failure_rejects_the_read_and_keeps_the_last_snapshot() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    runtime.ingest(&update("codex", 30), &Mapper).unwrap();
    let read = request_id(&runtime.read_usage().unwrap());
    answer(
        &mut runtime,
        read,
        json!({"error":{"code":-32603,"message":"failed to fetch codex rate limits"}}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    assert_eq!(used(&runtime), vec![30]);
}

#[test]
fn no_read_is_sent_until_an_abandoned_one_answers() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    runtime.ingest(&update("codex", 30), &Mapper).unwrap();
    let a = request_id(&runtime.read_usage().unwrap());
    runtime.abandon_passive_read();
    assert!(!runtime.native_rpc_pending());

    assert!(runtime.read_usage().is_err());
    assert_eq!(used(&runtime), vec![30]);
    answer(
        &mut runtime,
        a,
        json!({"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":9}}}}),
    );
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert_eq!(used(&runtime), vec![30]);

    let c = request_id(&runtime.read_usage().unwrap());
    answer(
        &mut runtime,
        c,
        json!({"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":11}}}}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    assert_eq!(used(&runtime), vec![11]);
}

#[test]
fn a_pushed_snapshot_lets_a_new_read_past_an_unanswered_one() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let abandoned = request_id(&runtime.read_usage().unwrap());
    runtime.abandon_passive_read();
    assert!(runtime.read_usage().is_err());

    runtime.ingest(&update("codex", 30), &Mapper).unwrap();
    let next = request_id(&runtime.read_usage().unwrap());
    answer(
        &mut runtime,
        abandoned,
        json!({"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":9}}}}),
    );
    assert_eq!(runtime.take_native_rpc_reply(), None);
    assert!(runtime.native_rpc_pending());
    answer(
        &mut runtime,
        next,
        json!({"result":{"rateLimits":{"limitId":"codex","primary":{"usedPercent":31}}}}),
    );
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    assert_eq!(used(&runtime), vec![31]);
}

#[test]
fn a_chatgpt_sign_in_lifts_an_earlier_refusal() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let read = request_id(&runtime.read_usage().unwrap());
    answer(
        &mut runtime,
        read,
        json!({"error":{"code":-32600,"message":"codex account authentication required"}}),
    );
    assert_eq!(runtime.usage().unwrap().state(), UsageState::Unsupported);
    for (mode, state) in [
        (json!("apikey"), UsageState::Unsupported),
        (json!("chatgpt"), UsageState::Unavailable),
    ] {
        let signed_in = frame(json!({"method":"account/updated",
            "params":{"authMode":mode,"planType":null}}));
        runtime.ingest(&signed_in, &Mapper).unwrap();
        assert_eq!(runtime.usage().unwrap().state(), state);
    }
}
