use super::*;
use crate::acp::goal::NativeRpcReply;

fn update(used: i64) -> Frame {
    frame(
        json!({"method":"account/rateLimits/updated","params":{"rateLimits":{
            "limitId":"codex","limitName":null,"planType":"plus",
            "primary":{"usedPercent":used,"windowDurationMins":300,"resetsAt":1_762_900_000},
            "secondary":null,"credits":null,"rateLimitReachedType":null
        }}}),
    )
}

fn used(runtime: &CodexRuntime) -> Vec<u8> {
    let snapshot = runtime.usage().unwrap().snapshot().unwrap();
    snapshot.windows.iter().map(|w| w.used_percent).collect()
}

#[test]
fn pushed_usage_replaces_the_previous_snapshot_without_transcript_events() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    assert_eq!(runtime.usage().unwrap().snapshot(), None);
    for percent in [12, 57] {
        let batch = runtime.ingest(&update(percent), &Mapper).unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    }
    assert_eq!(used(&runtime), vec![57]);
    let bogus = frame(json!({"method":"account/rateLimits/updated","params":{"rateLimits":{}}}));
    runtime.ingest(&bogus, &Mapper).unwrap();
    assert_eq!(used(&runtime), vec![57]);
}

#[test]
fn usage_read_answers_the_native_rpc_and_a_refusal_keeps_the_last_snapshot() {
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    runtime.ingest(&update(30), &Mapper).unwrap();

    let read = runtime.read_usage().unwrap();
    let wire: Value = serde_json::from_slice(read.frame().payload()).unwrap();
    assert_eq!(wire["method"], "account/rateLimits/read");
    assert!(runtime.native_rpc_pending());
    assert!(runtime.read_usage().is_err());
    runtime
        .ingest(
            &frame(json!({"id":wire["id"],"result":{
                "rateLimits":{"limitId":"codex","primary":{"usedPercent":64}},
                "rateLimitsByLimitId":null,"ordinaryUsageAllowed":true
            }})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::UsageRead)
    );
    assert!(!runtime.native_rpc_pending());
    assert_eq!(used(&runtime), vec![64]);

    let read = runtime.read_usage().unwrap();
    let id = request_id(&read);
    runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32600,"message":"chatgpt authentication required"}})),
            &Mapper,
        )
        .unwrap();
    assert_eq!(
        runtime.take_native_rpc_reply(),
        Some(NativeRpcReply::Rejected)
    );
    assert_eq!(used(&runtime), vec![64]);
}
