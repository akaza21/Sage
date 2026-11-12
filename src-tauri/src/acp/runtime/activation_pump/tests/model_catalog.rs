use crate::acp::bindings::codex::test_identity::{MANAGED_CODEX_HOME, user_agent};
use crate::acp::bindings::codex::{CodexRuntime, Platform};
use serde_json::{Value, json};

use super::*;

fn initialize_result() -> Value {
    json!({"userAgent":user_agent("sage", "unknown"),
        "codexHome":MANAGED_CODEX_HOME,
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

struct IdleCatalogTransport(usize);

impl crate::acp::runtime::EgressTransport for IdleCatalogTransport {
    fn write(&mut self, _frame: &crate::acp::transport::OutboundFrame) -> FrameWriteOutcome {
        self.0 += 1;
        FrameWriteOutcome::Complete
    }
}

#[test]
fn idle_catalog_accepts_exact_shared_model_limit_and_rejects_one_more_before_mutation_or_egress() {
    let advertised = capabilities(true, true);
    let activation = intent(ActivationMode::New {
        native_root: r"C:\repo".into(),
        model: None,
    });
    let model = |index| AdapterModel {
        id: ModelId::new("modelId", format!("model-{index}")).unwrap(),
        name: RequiredText::new("modelName", format!("Model {index}")).unwrap(),
        is_default: false,
    };
    let limit = LIMITS.wire.models_or_config_entries as usize;
    let session = ActivatedSession {
        binding_digest: activation.binding_digest,
        project_id: activation.project_id,
        chat_id: activation.chat_id,
        session_id: activation.session_id,
        external_session_id: ExternalId::new("thread", "thread-catalog").unwrap(),
        runtime_epoch: activation.runtime_epoch,
        capabilities: advertised.clone(),
        models: (0..limit - 1).map(model).collect(),
        next_model_cursor: None,
    };
    let mut idle =
        IdleActivationParts::new(adapter(advertised, vec![]), (), NoResources(17), session);
    let mut transport = IdleCatalogTransport(0);
    idle.accept_idle_batch(
        AdapterBatch::new(
            vec![AdapterEvent::Models {
                models: vec![model(limit - 1)],
                next_cursor: Some(PageCursor::new("cursor", "next").unwrap()),
            }],
            vec![],
        )
        .unwrap(),
        &mut transport,
    )
    .unwrap();
    assert_eq!(
        idle.session().models,
        (0..limit).map(model).collect::<Vec<_>>()
    );
    assert_eq!(
        idle.session().next_model_cursor.as_ref().unwrap().as_str(),
        "next"
    );
    let retained = idle.session().clone();
    let over = AdapterBatch::new(
        vec![AdapterEvent::Models {
            models: vec![model(limit)],
            next_cursor: Some(PageCursor::new("cursor", "over").unwrap()),
        }],
        vec![AdapterEgress::new(
            crate::acp::transport::OutboundFrame::new(b"{\"id\":1}".to_vec()).unwrap(),
            EgressToken::request(1),
        )],
    )
    .unwrap();
    assert!(matches!(
        idle.accept_idle_batch(over, &mut transport),
        Err(crate::acp::runtime::AdapterPumpError::BatchCapacity)
    ));
    assert_eq!(idle.session(), &retained);
    assert_eq!(transport.0, 0);
    assert!(idle.adapter.resolutions.lock().unwrap().is_empty());
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
                            rewind_before: None,
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
                    assert!(pump.adapter.session_config().is_none());
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
