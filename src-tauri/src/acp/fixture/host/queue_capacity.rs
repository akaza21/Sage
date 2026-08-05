use std::time::{Duration, Instant};

use serde_json::{Value, json};

use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
        SageServerRequestId, SageTurnId,
    },
    fixture::generated_cases::FrozenCase,
    intent::{ExternalId, Prompt},
    limits::LIMITS,
    runtime::{
        CancelIntent, DriverMailbox, DriverMailboxError, JournalError, JournalHandle, JournalLane,
        permissions::PermissionIntent,
    },
    store::{AcpStore, StoreCommit, StoreError, StoreMutation},
    transport::{EgressQueue, OutboundFrame, QueueError},
};

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "driver-data-messages-256",
    "driver-data-messages-257",
    "driver-data-bytes-2097152",
    "driver-data-bytes-2097153",
    "egress-frames-64",
    "egress-frames-65",
    "egress-data-cannot-consume-control-reserve",
    "egress-bytes-2097152",
    "egress-bytes-2097153",
    "journal-batches-128",
    "journal-batches-129",
    "journal-data-cannot-consume-control-reserve",
    "journal-bytes-4194304",
    "journal-bytes-4194305",
];
pub(in crate::acp::fixture) const CANCEL_CASE_IDS: &[&str] = &["cancel-dispatch-at-capacity"];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "cancel-dispatch-at-capacity" => execute_cancel(&steps),
        id if id.starts_with("driver-") => execute_driver(id, &steps),
        id if id.starts_with("egress-") => execute_egress(id, &steps),
        id if id.starts_with("journal-") => execute_journal(id, &steps),
        id => panic!("unexpected queue-capacity case {id}"),
    }
}

fn execute_cancel(steps: &[Value]) -> Value {
    let [fill, dispatch] = steps else {
        panic!("expected exact driver fill followed by cancel dispatch");
    };
    assert_eq!(fill["op"], "fillDriverData");
    assert_eq!(dispatch["op"], "dispatchCancel");
    let messages = field_u64(fill, "messages");
    let bytes = field_u64(fill, "bytes");
    let literal_deadline_ms = field_u64(dispatch, "deadlineMs");
    let production_deadline_ms = LIMITS.deadlines_ms.cancel_dispatch;
    assert_eq!(literal_deadline_ms, production_deadline_ms);
    assert_eq!(messages, LIMITS.queues.driver_data_messages);
    assert_eq!(bytes, messages);

    let mailbox = DriverMailbox::new().unwrap();
    for _ in 0..messages {
        mailbox.try_push_chunk(vec![b'x']).unwrap();
    }
    let saturated_before = mailbox.try_push_chunk(vec![b'x']).is_err();
    let intent = cancel_intent();
    let started = Instant::now();
    mailbox
        .submit_cancel(intent.clone(), production_deadline_ms)
        .unwrap();
    let saturated_with_control = mailbox.try_push_chunk(vec![b'x']).is_err();
    let observed_intent = mailbox
        .try_pop_cancel_for_fixture()
        .expect("reserved cancel envelope");
    let elapsed = started.elapsed();
    assert_eq!(observed_intent, intent);
    assert!(elapsed <= Duration::from_millis(literal_deadline_ms));

    json!({
        "cancelDispatchMsMaximum": production_deadline_ms,
        "controlCapacityBorrowed": !(saturated_before && saturated_with_control),
    })
}

fn cancel_intent() -> CancelIntent {
    CancelIntent {
        project_id: SageProjectId::parse("fixture-project").unwrap(),
        chat_id: SageChatId::parse("fixture-chat").unwrap(),
        session_id: SageAgentSessionId::parse("fixture-session").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse("fixture-turn").unwrap(),
        client_message_id: RequiredText::new("clientMessageId", "fixture-turn").unwrap(),
    }
}

fn execute_driver(id: &str, steps: &[Value]) -> Value {
    let mailbox = DriverMailbox::new().unwrap();
    let step = one_step(steps, "fillDriverData");
    if let Some(messages) = step.get("messages").and_then(Value::as_u64) {
        let bytes = field_u64(step, "bytes");
        assert_eq!(messages, bytes);
        for index in 0..messages {
            let result = mailbox.try_push_chunk(vec![b'x']);
            if result.is_err() {
                assert_eq!(index, 256);
                assert_eq!(result, Err(DriverMailboxError::Full));
                return json!({
                    "accepted": false,
                    "error": "RuntimeCapacity",
                    "retainedMessagesMaximum": 256,
                });
            }
        }
        return json!({"accepted": true, "retainedMessages": messages});
    }

    let repeated = step["repeatedMessageBytes"].as_object().unwrap();
    let bytes = repeated["bytes"].as_u64().unwrap();
    let count = repeated["count"].as_u64().unwrap();
    for _ in 0..count {
        mailbox.try_push_chunk(vec![b'x'; bytes as usize]).unwrap();
    }
    let retained = bytes.checked_mul(count).unwrap();
    if id.ends_with("2097152") {
        return json!({"accepted": true, "retainedBytes": retained});
    }
    let additional = step["additionalMessageBytes"].as_array().unwrap();
    let kinds = step["additionalMessageKinds"].as_array().unwrap();
    assert_eq!(additional.len(), 1);
    assert_eq!(kinds, &[json!("prompt")]);
    let actual_bytes = additional[0].as_u64().unwrap();
    let prompt_bytes = actual_bytes.checked_sub(1).unwrap();
    let result = mailbox.submit_prompt(
        Prompt::new("prompt", "p".repeat(prompt_bytes as usize)).unwrap(),
        RequiredText::new("message", "m").unwrap(),
    );
    assert!(matches!(result, Err(DriverMailboxError::Full)));
    json!({
        "accepted": false,
        "error": "RuntimeCapacity",
        "retainedBytesMaximum": 2097152,
        "messageLimitBreached": false,
    })
}

fn execute_egress(id: &str, steps: &[Value]) -> Value {
    let mut queue = EgressQueue::new().unwrap();
    let mut accepted_frames = 0_u64;
    let mut accepted_bytes = 0_u64;
    let mut rejected = false;
    for step in steps {
        let control = match step["op"].as_str().unwrap() {
            "fillEgressData" => false,
            "fillEgressControl" => true,
            operation => panic!("unexpected egress operation {operation}"),
        };
        let frames = field_u64(step, "frames");
        let total = field_u64(step, "bytes");
        let sizes = distribute_sizes(step, frames, total, 3);
        for wire_bytes in sizes {
            let frame = frame_with_wire_bytes(wire_bytes as usize);
            let result = if control {
                queue.try_push_control(frame, accepted_frames)
            } else {
                queue.try_push_data(frame)
            };
            match result {
                Ok(()) => {
                    accepted_frames += 1;
                    accepted_bytes += wire_bytes;
                }
                Err(QueueError::CountCapacity | QueueError::ByteCapacity) => rejected = true,
                Err(error) => panic!("unexpected egress error {error:?}"),
            }
        }
    }

    match id {
        "egress-frames-64" => {
            assert!(!rejected);
            json!({"accepted": true, "totalFrames": accepted_frames, "controlReserveUsable": true})
        }
        "egress-frames-65" => {
            assert!(rejected);
            json!({"accepted": false, "error": "RuntimeCapacity", "totalFramesMaximum": 64})
        }
        "egress-data-cannot-consume-control-reserve" => {
            assert!(rejected);
            queue
                .try_push_control(frame_with_wire_bytes(3), 0)
                .expect("reserved control capacity");
            json!({
                "accepted": false,
                "dataFramesMaximum": 56,
                "dataBytesMaximum": 1835008,
                "controlReserveIntact": true,
            })
        }
        "egress-bytes-2097152" => {
            assert!(!rejected);
            json!({"accepted": true, "totalBytes": accepted_bytes})
        }
        "egress-bytes-2097153" => {
            assert!(rejected);
            json!({"accepted": false, "totalBytesMaximum": 2097152})
        }
        _ => unreachable!(),
    }
}

fn execute_journal(id: &str, steps: &[Value]) -> Value {
    let (handle, _runner) = JournalHandle::new(RejectingStore);
    let mut held_batches = Vec::new();
    let mut accepted_count = 0_u64;
    let mut accepted_bytes = 0_u64;
    let mut rejected = false;
    for step in steps {
        let lane = match step["op"].as_str().unwrap() {
            "fillJournalData" => JournalLane::Data,
            "fillJournalControl" => JournalLane::Control,
            operation => panic!("unexpected journal operation {operation}"),
        };
        let batches = field_u64(step, "batches");
        let sizes = match step.get("bytes").and_then(Value::as_u64) {
            Some(total) => distribute_sizes(step, batches, total, 1),
            None => vec![0; batches as usize],
        };
        for bytes in sizes {
            match handle.batch(lane) {
                Ok(mut batch) => {
                    if bytes != 0 {
                        let mutation = mutation_with_retained_bytes(bytes as usize);
                        if let Err(error) = batch.push(mutation) {
                            assert!(matches!(error, JournalError::Capacity { .. }));
                            rejected = true;
                            held_batches.push(batch);
                            continue;
                        }
                    }
                    held_batches.push(batch);
                    accepted_count += 1;
                    accepted_bytes += bytes;
                }
                Err(JournalError::Capacity { .. }) => rejected = true,
                Err(error) => panic!("unexpected journal error {error:?}"),
            }
        }
    }

    match id {
        "journal-batches-128" => {
            assert!(!rejected);
            json!({"accepted": true, "totalBatches": accepted_count, "controlReserveUsable": true})
        }
        "journal-batches-129" => {
            assert!(rejected);
            json!({"accepted": false, "totalBatchesMaximum": 128})
        }
        "journal-data-cannot-consume-control-reserve" => {
            assert!(rejected);
            held_batches.push(handle.batch(JournalLane::Control).unwrap());
            json!({
                "accepted": false,
                "dataBatchesMaximum": 120,
                "controlReserveIntact": true,
            })
        }
        "journal-bytes-4194304" => {
            assert!(!rejected);
            json!({"accepted": true, "totalBytes": accepted_bytes})
        }
        "journal-bytes-4194305" => {
            assert!(rejected);
            json!({"accepted": false, "totalBytesMaximum": 4194304})
        }
        _ => unreachable!(),
    }
}

fn mutation_with_retained_bytes(target: usize) -> StoreMutation {
    let build = |payload: String| StoreMutation::ReceivePermission {
        intent: PermissionIntent {
            request_id: SageServerRequestId::parse("fixture-request").unwrap(),
            project_id: SageProjectId::parse("fixture-project").unwrap(),
            chat_id: SageChatId::parse("fixture-chat").unwrap(),
            session_id: SageAgentSessionId::parse("fixture-session").unwrap(),
            turn_id: SageTurnId::parse("fixture-turn").unwrap(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            external_request_id: ExternalId::new("externalRequest", "fixture-request").unwrap(),
            kind: RequiredText::new("requestKind", "permission").unwrap(),
            payload_digest: "b".repeat(64),
            normalized_payload_digest: "c".repeat(64),
            normalized_payload_json: payload,
        },
        now_millis: 1,
    };
    let base = build("\"\"".into());
    let base_bytes = JournalHandle::fixture_mutation_retained_bytes(&base);
    assert!(target >= base_bytes);
    let mutation = build(format!("\"{}\"", "x".repeat(target - base_bytes)));
    assert_eq!(
        JournalHandle::fixture_mutation_retained_bytes(&mutation),
        target
    );
    mutation
}

fn distribute_sizes(step: &Value, count: u64, total: u64, minimum: u64) -> Vec<u64> {
    if let Some(each) = step.get("bytesEach").and_then(Value::as_u64) {
        assert_eq!(each.checked_mul(count).unwrap(), total);
        return vec![each; count as usize];
    }
    if let Some(largest) = step.get("largestFrameBytes").and_then(Value::as_u64) {
        return repeated_then_largest(count, total, largest);
    }
    if let Some(largest) = step.get("largestBatchBytes").and_then(Value::as_u64) {
        return repeated_then_largest(count, total, largest);
    }
    assert!(total >= count * minimum);
    let base = total / count;
    let extra = total % count;
    (0..count)
        .map(|index| base + u64::from(index < extra))
        .collect()
}

fn repeated_then_largest(count: u64, total: u64, largest: u64) -> Vec<u64> {
    assert!(count > 1);
    let repeated = total.checked_sub(largest).unwrap() / (count - 1);
    assert_eq!(repeated * (count - 1) + largest, total);
    let mut sizes = vec![repeated; count as usize - 1];
    sizes.push(largest);
    sizes
}

fn frame_with_wire_bytes(bytes: usize) -> OutboundFrame {
    assert!(bytes >= 3);
    let mut payload = Vec::with_capacity(bytes - 1);
    payload.push(b'{');
    payload.resize(bytes - 2, b' ');
    payload.push(b'}');
    let frame = OutboundFrame::new(payload).unwrap();
    assert_eq!(frame.wire_len(), bytes);
    frame
}

fn one_step<'a>(steps: &'a [Value], operation: &str) -> &'a Value {
    let [step] = steps else {
        panic!("expected one fixture step");
    };
    assert_eq!(step["op"], operation);
    step
}

fn field_u64(value: &Value, field: &str) -> u64 {
    value[field]
        .as_u64()
        .unwrap_or_else(|| panic!("missing numeric field {field}"))
}

struct RejectingStore;

impl AcpStore for RejectingStore {
    fn commit(&mut self, _batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
        Err(StoreError::BatchShape)
    }
}

#[test]
fn corrected_cancel_capacity_case_matches_its_literal_oracle() {
    for case_id in super::CANCEL_CASE_IDS {
        let case = super::super::generated_cases::FROZEN_CASES
            .iter()
            .find(|case| case.id == *case_id)
            .unwrap();
        super::super::oracle::assert_literal(case, execute(case));
    }
}
