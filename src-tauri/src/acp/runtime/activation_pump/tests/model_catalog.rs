use crate::acp::bindings::codex::{CodexRuntime, Platform};
use serde_json::{Value, json};

use super::*;

fn initialize_result() -> Value {
    json!({"userAgent":"sage/0.146.0 (Windows 10.0.26200; x86_64) unknown (sage; 0.1.0)",
        "codexHome":"C:\\SageHarnesses\\CodexState\\codex-app-server-0.146.0-d3992fec",
        "platformFamily":"windows","platformOs":"windows"})
}

fn frame(value: Value) -> Frame {
    crate::acp::transport::IncrementalFrameDecoder::new()
        .push_chunk(&line(value))
        .unwrap()
        .unwrap()
}

fn line(value: Value) -> Vec<u8> {
    let mut bytes = serde_json::to_vec(&value).unwrap();
    bytes.push(b'\n');
    bytes
}

fn request_id(egress: &AdapterEgress) -> u64 {
    serde_json::from_slice::<Value>(egress.frame().payload()).unwrap()["id"]
        .as_u64()
        .unwrap()
}

#[test]
fn catalog_error_before_activation_preserves_empty_or_partial_catalog_and_live_session() {
    runtime().block_on(async {
        for partial in [false, true] {
            let mut seed = CodexRuntime::fixture(Platform::WindowsX86_64);
            let initialize = seed.command(AdapterCommand::Initialize).unwrap();
            seed.ingest(
                &frame(json!({"id":request_id(&initialize),"result":initialize_result()})),
                &NoResources(17),
            )
            .unwrap();
            let capabilities = seed.capabilities().unwrap();
            let mut activation_intent = intent_with_epoch(
                ActivationMode::New {
                    native_root: r"C:\repo".into(),
                    model: Some(ModelId::new("model", "selected").unwrap()),
                },
                1,
            );
            activation_intent.curated_ceiling = capabilities.clone();
            activation_intent.sage_policy = capabilities;
            let adapter =
                CodexRuntime::fixture_with_identity(Platform::WindowsX86_64, [7; 32], session_id());
            let (journal, commands) = journal(Ok(ActivationReceipt {
                projection_revision: ProjectionRevision::new(1).unwrap(),
            }));
            let mut pump =
                ActivationPump::new(adapter, journal, NoResources(17), activation_intent).unwrap();
            let mut transport = Transport(VecDeque::new());
            pump.begin().unwrap();
            pump.flush_one(&mut transport).unwrap();
            pump.push_stdout_chunk(&line(json!({"id":1,"result":initialize_result()})), 1)
                .await
                .unwrap();
            pump.flush_one(&mut transport).unwrap();
            pump.flush_one(&mut transport).unwrap();
            let page = pump
                .adapter
                .command(AdapterCommand::ModelPage { cursor: None })
                .unwrap();
            let mut page_id = request_id(&page);
            pump.adapter
                .resolve_egress(page.into_token(), EgressResolution::Written)
                .unwrap();
            if partial {
                pump.push_stdout_chunk(&line(json!({"id":page_id,"result":{
                    "data":[{"id":"selected","displayName":"Selected","isDefault":false,
                        "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                            {"reasoningEffort":"high","description":"Deep"}]}],"nextCursor":"second"
                }})), 2).await.unwrap();
                let page = pump
                    .adapter
                    .command(AdapterCommand::ModelPage {
                        cursor: Some(PageCursor::new("cursor", "second").unwrap()),
                    })
                    .unwrap();
                page_id = request_id(&page);
                pump.adapter
                    .resolve_egress(page.into_token(), EgressResolution::Written)
                    .unwrap();
            }
            let retained_config = pump.adapter.session_config();
            pump.push_stdout_chunk(
                &line(json!({"id":page_id,"error":{
                    "code":-32601,"message":"Method not found"
                }})),
                3,
            )
            .await
            .unwrap();
            assert!(!pump.teardown_required());
            assert_eq!(pump.adapter.session_config(), retained_config);
            assert_eq!(pump.models.len(), if partial { 1 } else { 0 });
            assert!(commands.lock().unwrap().is_empty());
            pump.push_stdout_chunk(
                &line(json!({"id":2,"result":{
                    "thread":{"id":"thread-catalog"}
                }})),
                4,
            )
            .await
            .unwrap();
            let idle = pump
                .try_into_idle_parts()
                .ok()
                .expect("live activated session");
            assert_eq!(idle.session().models.len(), if partial { 1 } else { 0 });
            assert_eq!(commands.lock().unwrap().len(), 1);
        }
    });
}

#[test]
fn invalid_optional_thread_model_and_effort_do_not_fail_start_or_resume_activation() {
    runtime().block_on(async {
        for resume in [false, true] {
            for field in ["model", "reasoningEffort"] {
                for value in [json!(""), json!("x".repeat(513)), json!(42), json!({})] {
                    let mut seed = CodexRuntime::fixture(Platform::WindowsX86_64);
                    let initialize = seed.command(AdapterCommand::Initialize).unwrap();
                    seed.ingest(
                        &frame(json!({"id":request_id(&initialize),"result":initialize_result()})),
                        &NoResources(17),
                    )
                    .unwrap();
                    let capabilities = seed.capabilities().unwrap();
                    let mode = if resume {
                        ActivationMode::Resume {
                            native_root: r"C:\repo".into(),
                            external_session_id: ExternalId::new("thread", "thread-catalog")
                                .unwrap(),
                            model: Some(ModelId::new("model", "selected").unwrap()),
                        }
                    } else {
                        ActivationMode::New {
                            native_root: r"C:\repo".into(),
                            model: Some(ModelId::new("model", "selected").unwrap()),
                        }
                    };
                    let mut activation_intent = intent_with_epoch(mode, 1);
                    activation_intent.curated_ceiling = capabilities.clone();
                    activation_intent.sage_policy = capabilities;
                    let adapter = CodexRuntime::fixture_with_identity(
                        Platform::WindowsX86_64,
                        [7; 32],
                        session_id(),
                    );
                    let (journal, commands) = journal(Ok(ActivationReceipt {
                        projection_revision: ProjectionRevision::new(1).unwrap(),
                    }));
                    let mut pump =
                        ActivationPump::new(adapter, journal, NoResources(17), activation_intent)
                            .unwrap();
                    let mut transport = Transport(VecDeque::new());
                    pump.begin().unwrap();
                    pump.flush_one(&mut transport).unwrap();
                    pump.push_stdout_chunk(&line(json!({"id":1,"result":initialize_result()})), 1)
                        .await
                        .unwrap();
                    pump.flush_one(&mut transport).unwrap();
                    pump.flush_one(&mut transport).unwrap();
                    let mut result = json!({"thread":{"id":"thread-catalog"}});
                    result[field] = value;
                    pump.push_stdout_chunk(&line(json!({"id":2,"result":result})), 2)
                        .await
                        .unwrap();
                    assert!(!pump.teardown_required());
                    assert!(pump.adapter.session_config().is_none());
                    let model_id = pump
                        .adapter
                        .command(AdapterCommand::ModelPage { cursor: None })
                        .unwrap();
                    pump.adapter
                        .ingest(
                            &frame(json!({"id":request_id(&model_id),"result":{
                                "data":[{"id":"selected","displayName":"Selected","isDefault":false,
                                    "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                                        {"reasoningEffort":"high","description":"Deep"}]}]
                            }})),
                            &NoResources(17),
                        )
                        .unwrap();
                    assert_eq!(pump.adapter.session_config().is_some(), !resume);
                    let idle = pump.try_into_idle_parts().ok().expect("activated session");
                    assert_eq!(
                        idle.session().external_session_id.as_str(),
                        "thread-catalog"
                    );
                    assert_eq!(commands.lock().unwrap().len(), 1);
                }
            }
        }
    });
}
