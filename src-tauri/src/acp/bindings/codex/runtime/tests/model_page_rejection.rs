use super::*;
use crate::acp::lifecycle::TurnState;

#[test]
fn invalid_catalog_discard_requires_exact_pending_read_only_response() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let list_id = request_id(&runtime.model_config_hydration().unwrap().unwrap());
    let data = (0..513)
        .map(|index| {
            json!({
                "id":format!("model-{index}"),"displayName":"Model"
            })
        })
        .collect::<Vec<_>>();
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "continue").unwrap(),
            client_message_id: RequiredText::new("client", "catalog-discard").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    let prompt_id = request_id(&prompt);
    for value in [
        json!({"id":list_id+1000,"result":{"data":data}}),
        json!({"id":prompt_id,"result":{"data":data}}),
        json!({"id":list_id,"method":"turn/started","result":{"data":data}}),
    ] {
        assert!(matches!(
            runtime.discard_invalid_response(&frame(value)),
            Err(AdapterRuntimeError::Frame)
        ));
    }
    let batch = runtime
        .discard_invalid_response(&frame(json!({
            "id":list_id,"result":{"data":data,"nextCursor":"next"}
        })))
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    assert!(runtime.model_config_hydration().unwrap().is_none());
    let batch = runtime
        .ingest(
            &frame(json!({
                "id":prompt_id,"result":{"turn":{"id":"turn-discard"}}
            })),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        batch.events(),
        [AdapterEvent::Turn {
            state: TurnState::Sending,
            ..
        }]
    ));
}

pub(super) fn runtime_waiting_for_effort_page(partial: bool) -> (CodexRuntime, u64) {
    let mut runtime = fixture_runtime();
    ready_with_reported_config(&mut runtime, Some("model"), Some("high"));
    let list = runtime.model_config_hydration().unwrap().unwrap();
    let list_id = request_id(&list);
    runtime
        .resolve_egress(list.into_token(), EgressResolution::Written)
        .unwrap();
    if !partial {
        return (runtime, list_id);
    }
    let batch = runtime
        .ingest(
            &frame(json!({"id":list_id,"result":{
                "data":[{"id":"model","displayName":"Model","isDefault":true,
                    "defaultReasoningEffort":"high","supportedReasoningEfforts":[
                        {"reasoningEffort":"high","description":"Deep"}]}],"nextCursor":"second"
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(batch.events(), [AdapterEvent::Models { models, .. }] if models.len() == 1));
    let next = batch.egress().first().expect("next catalog request");
    let next_id = request_id(next);
    runtime
        .resolve_egress(next.token().clone(), EgressResolution::Written)
        .unwrap();
    assert_eq!(runtime.model_catalog_entries, 1);
    assert!(runtime.session_config().is_some());
    (runtime, next_id)
}

pub(super) fn assert_invalid_model_page_is_ignored(
    runtime: &mut CodexRuntime,
    list_id: u64,
    result: Value,
) {
    let selected_model = runtime.selected_model.clone();
    let selected_model_id = runtime.selected_model_id.clone();
    let selected_effort = runtime.selected_effort.clone();
    let config = runtime.session_config();
    let catalog_size = (runtime.model_catalog_entries, runtime.model_catalog_bytes);
    let batch = runtime
        .ingest(&frame(json!({"id":list_id,"result":result})), &Mapper)
        .expect("invalid catalog page is nonfatal");
    assert!(matches!(batch.events(), [AdapterEvent::IgnoredLate]));
    assert!(batch.egress().is_empty());
    assert_eq!(runtime.selected_model, selected_model);
    assert_eq!(runtime.selected_model_id, selected_model_id);
    assert_eq!(runtime.selected_effort, selected_effort);
    assert_eq!(runtime.session_config(), config);
    assert_eq!(
        (runtime.model_catalog_entries, runtime.model_catalog_bytes),
        catalog_size
    );
    assert!(!runtime.poisoned);
    assert!(runtime.model_config_hydration().unwrap().is_none());
    assert!(
        runtime
            .command(AdapterCommand::Prompt {
                prompt: Prompt::new("prompt", "continue after invalid catalog page").unwrap(),
                client_message_id: RequiredText::new("client", "invalid-catalog-page").unwrap(),
                access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
            })
            .is_ok()
    );
}
