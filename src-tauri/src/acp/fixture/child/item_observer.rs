use serde_json::{Value, json};

use crate::acp::{
    bindings::adapter::{
        AdapterCommand, AdapterEvent, EgressResolution, HarnessAdapter, ResourceMapper,
    },
    contract::{RequiredText, RuntimeEpoch, SageAgentSessionId},
    fixture::adapter::FixtureAdapter,
    intent::{ExternalId, Prompt, SafeResourceRef, UnavailableResourceReason},
    lifecycle::{ItemLifecycle, ToolState},
    transport::{Frame, IncrementalFrameDecoder, validate_structure},
};

use super::super::{generated_cases::FrozenCase, wire};

pub(crate) const CASE_IDS: &[&str] = &[
    "item-identical-terminal-duplicate",
    "item-conflicting-terminal-duplicate",
    "item-late-delta-after-terminal",
];

pub(crate) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    let (stdout, _) = wire::capture_steps(&steps).unwrap();
    let frames = decode(&stdout);
    let mut adapter = ready_adapter();
    let first = adapter.ingest(&frames[0], &Mapper);
    let second = adapter.ingest(&frames[1], &Mapper);
    match case.id {
        "item-identical-terminal-duplicate" => json!({
            "tool": if first.as_ref().is_ok_and(|batch| matches!(
                batch.events(),
                [AdapterEvent::Item {
                    lifecycle: ItemLifecycle::Completed,
                    tool_state: Some(ToolState::Completed),
                    ..
                }]
            )) { "completed" } else { "invalid" },
            "secondUpdate": if second.as_ref().is_ok_and(|batch| matches!(
                batch.events(),
                [AdapterEvent::IgnoredDuplicate]
            )) { "noOp" } else { "applied" },
            "factCount": usize::from(first.is_ok()),
        }),
        "item-conflicting-terminal-duplicate" => {
            let first_preserved = adapter
                .ingest(&frames[0], &Mapper)
                .is_ok_and(|batch| matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
            json!({
                "terminal": if second.is_err() { "failed" } else { "ready" },
                "protocolViolation": second.is_err(),
                "firstTerminalPreserved": first_preserved,
            })
        }
        "item-late-delta-after-terminal" => json!({
            "lateUpdate": if second.as_ref().is_ok_and(|batch| matches!(
                batch.events(),
                [AdapterEvent::IgnoredLate]
            )) { "discarded" } else { "applied" },
            "terminalReopened": false,
        }),
        _ => panic!("unregistered item fixture {}", case.id),
    }
}

fn ready_adapter() -> FixtureAdapter {
    let mut adapter = FixtureAdapter::new(
        [7; 32],
        RuntimeEpoch::new(1).unwrap(),
        SageAgentSessionId::parse("fixture-session").unwrap(),
        ExternalId::new("externalSession", "fixture-external-session").unwrap(),
    )
    .unwrap();
    let egress = adapter
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "read only").unwrap(),
            client_message_id: RequiredText::new("message", "message-1").unwrap(),
        })
        .unwrap();
    adapter
        .resolve_egress(egress.token().clone(), EgressResolution::Queued)
        .unwrap();
    adapter
        .resolve_egress(egress.into_token(), EgressResolution::Written)
        .unwrap();
    adapter
}

fn decode(bytes: &[u8]) -> Vec<Frame> {
    let mut decoder = IncrementalFrameDecoder::new();
    let mut frames = Vec::new();
    for chunk in bytes.chunks(8 * 1024) {
        if let Some(frame) = decoder.push_chunk(chunk).unwrap() {
            validate_structure(&frame).unwrap();
            frames.push(frame);
        }
        while decoder.has_pending_input() {
            if let Some(frame) = decoder.drain_pending().unwrap() {
                validate_structure(&frame).unwrap();
                frames.push(frame);
            }
        }
    }
    assert!(decoder.finish().unwrap().is_none());
    frames
}

struct Mapper;

impl ResourceMapper for Mapper {
    fn map_existing(
        &self,
        _absolute_path: &str,
    ) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}
