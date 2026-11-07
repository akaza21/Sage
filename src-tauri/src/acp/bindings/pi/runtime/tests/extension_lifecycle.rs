use super::*;
use crate::acp::lifecycle::TurnState;

fn extension_runtime() -> PiRuntime {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    let query = runtime.discover_commands().unwrap().unwrap();
    let query = deliver(&mut runtime, query);
    runtime
        .ingest(
            &response(
                &query,
                json!({"commands":[{"name":"review","source":"extension"}]}),
            ),
            &Mapper,
        )
        .unwrap();
    runtime
}

fn extension(runtime: &mut PiRuntime, client: &str) -> AdapterEgress {
    runtime.command(extension_command(runtime, client)).unwrap()
}

fn extension_command(runtime: &PiRuntime, client: &str) -> AdapterCommand {
    let mut prompt = Prompt::new("prompt", "/review").unwrap();
    prompt.command_revision = Some(runtime.available_commands().snapshot().revision);
    AdapterCommand::Prompt {
        prompt,
        client_message_id: RequiredText::new("client", client).unwrap(),
        access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
    }
}

fn event(runtime: &mut PiRuntime, kind: &str) -> AdapterBatch {
    runtime
        .ingest(&frame(json!({"type":kind})), &Mapper)
        .unwrap()
}

fn assert_terminal(batch: &AdapterBatch, expected: TurnState) {
    let states: Vec<_> = batch
        .events()
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Turn { state, .. } if state.is_terminal() => Some(*state),
            _ => None,
        })
        .collect();
    assert_eq!(states, vec![expected]);
}

#[test]
fn stopped_async_extension_acknowledgment_settles_interrupted() {
    let mut runtime = extension_runtime();
    let send = extension(&mut runtime, "extension-a");
    let send = deliver(&mut runtime, send);
    let abort = runtime.command(AdapterCommand::Cancel).unwrap();
    let abort = deliver(&mut runtime, abort);
    runtime
        .ingest(&response(&abort, Value::Null), &Mapper)
        .unwrap();
    let batch = runtime
        .ingest(&response(&send, Value::Null), &Mapper)
        .unwrap();
    assert_terminal(&batch, TurnState::Interrupted);
}

fn reply(runtime: &mut PiRuntime, request: &Value, success: bool) -> AdapterBatch {
    runtime
        .ingest(
            &frame(json!({"type":"response", "id":request["id"],
                "command":request["type"], "success":success, "data":null})),
            &Mapper,
        )
        .unwrap()
}

fn assert_no_terminal(batch: &AdapterBatch) {
    assert!(
        !batch.events().iter().any(|event| {
            matches!(event, AdapterEvent::Turn { state, .. } if state.is_terminal())
        })
    );
}

fn reject_delivery(runtime: &mut PiRuntime, egress: AdapterEgress, resolution: EgressResolution) {
    let token = egress.into_token();
    if resolution == EgressResolution::WriteZeroBytes {
        runtime
            .resolve_egress(token.clone(), EgressResolution::Queued)
            .unwrap();
    }
    runtime.resolve_egress(token, resolution).unwrap();
}

#[test]
fn extension_acknowledgment_at_each_model_boundary_preserves_settlement() {
    for ack_after in 0..=2 {
        let mut runtime = extension_runtime();
        let send = extension(&mut runtime, "extension-a");
        let send = deliver(&mut runtime, send);
        event(&mut runtime, "agent_start");
        if ack_after == 0 {
            assert_no_terminal(&reply(&mut runtime, &send, true));
        }
        event(&mut runtime, "agent_end");
        if ack_after == 1 {
            assert_no_terminal(&reply(&mut runtime, &send, true));
        }
        // An ended model cycle may continue before the definitive settlement.
        event(&mut runtime, "agent_start");
        event(&mut runtime, "agent_end");
        assert_terminal(&event(&mut runtime, "agent_settled"), TurnState::Completed);
        if ack_after == 2 {
            assert_no_terminal(&reply(&mut runtime, &send, true));
        }
        assert_no_terminal(&reply(&mut runtime, &send, true));
        assert_no_terminal(&event(&mut runtime, "agent_settled"));
    }
}

#[test]
fn completed_extension_replies_cannot_affect_the_next_turn_or_discovery() {
    for success in [true, false] {
        for next_running in [false, true] {
            let mut runtime = extension_runtime();
            let first = extension(&mut runtime, "extension-a");
            let first = deliver(&mut runtime, first);
            let old_abort = runtime.command(AdapterCommand::Cancel).unwrap();
            let old_abort = deliver(&mut runtime, old_abort);
            event(&mut runtime, "agent_start");
            event(&mut runtime, "agent_end");
            assert_terminal(
                &event(&mut runtime, "agent_settled"),
                TurnState::Interrupted,
            );
            let query = runtime.discover_commands().unwrap().unwrap();
            let query = deliver(&mut runtime, query);
            let second = extension(&mut runtime, "extension-b");
            let second = deliver(&mut runtime, second);
            if next_running {
                event(&mut runtime, "agent_start");
            }
            let abort = runtime.command(AdapterCommand::Cancel).unwrap();
            let abort = deliver(&mut runtime, abort);
            reply(&mut runtime, &abort, true);
            for old in [&first, &old_abort] {
                assert!(matches!(
                    reply(&mut runtime, old, success).events(),
                    [AdapterEvent::IgnoredLate]
                ));
                assert_no_terminal(&reply(&mut runtime, old, success));
            }
            runtime
                .ingest(&response(&query, json!({"commands":[]})), &Mapper)
                .unwrap();
            assert!(runtime.available_commands().snapshot().commands.is_empty());
            let ack = reply(&mut runtime, &second, true);
            if next_running {
                assert_no_terminal(&ack);
                event(&mut runtime, "agent_end");
                assert_terminal(
                    &event(&mut runtime, "agent_settled"),
                    TurnState::Interrupted,
                );
            } else {
                assert_terminal(&ack, TurnState::Interrupted);
            }
        }
    }
}

#[test]
fn abort_at_each_model_stage_survives_later_acknowledgment() {
    for abort_after in 0..=3 {
        let mut runtime = extension_runtime();
        let send = extension(&mut runtime, "extension-a");
        let send = deliver(&mut runtime, send);
        for stage in 0..=3 {
            match stage {
                1 => {
                    event(&mut runtime, "agent_start");
                }
                2 => {
                    assert_no_terminal(&reply(&mut runtime, &send, true));
                }
                3 => {
                    event(&mut runtime, "agent_end");
                }
                _ => {}
            }
            if stage == abort_after {
                let abort = runtime.command(AdapterCommand::Cancel).unwrap();
                deliver(&mut runtime, abort);
            }
        }
        assert_terminal(
            &event(&mut runtime, "agent_settled"),
            TurnState::Interrupted,
        );
        assert!(runtime.command(AdapterCommand::Cancel).is_err());
        assert_no_terminal(&event(&mut runtime, "agent_settled"));
    }
}

#[test]
fn rejected_abort_does_not_clear_another_stop_request() {
    for first_accepted in [false, true] {
        for reject_write in [false, true] {
            let mut runtime = extension_runtime();
            let send = extension(&mut runtime, "extension-a");
            let send = deliver(&mut runtime, send);
            let first = runtime.command(AdapterCommand::Cancel).unwrap();
            let first = deliver(&mut runtime, first);
            if first_accepted {
                reply(&mut runtime, &first, true);
            }
            let second = runtime.command(AdapterCommand::Cancel).unwrap();
            if reject_write {
                reject_delivery(&mut runtime, second, EgressResolution::WriteZeroBytes);
            } else {
                let second = deliver(&mut runtime, second);
                reply(&mut runtime, &second, false);
            }
            assert_terminal(&reply(&mut runtime, &send, true), TurnState::Interrupted);
        }
    }
}

#[test]
fn sole_rejected_stop_allows_successful_extension_completion() {
    for resolution in [
        EgressResolution::QueueRejected,
        EgressResolution::WriteZeroBytes,
    ] {
        let mut runtime = extension_runtime();
        let send = extension(&mut runtime, "extension-a");
        let send = deliver(&mut runtime, send);
        let abort = runtime.command(AdapterCommand::Cancel).unwrap();
        reject_delivery(&mut runtime, abort, resolution);
        assert_terminal(&reply(&mut runtime, &send, true), TurnState::Completed);
    }
    let mut runtime = extension_runtime();
    let send = extension(&mut runtime, "extension-a");
    let send = deliver(&mut runtime, send);
    let abort = runtime.command(AdapterCommand::Cancel).unwrap();
    let abort = deliver(&mut runtime, abort);
    reply(&mut runtime, &abort, false);
    assert_terminal(&reply(&mut runtime, &send, true), TurnState::Completed);
}

#[test]
fn pending_completed_extension_acknowledgments_fail_closed_at_capacity_and_recover() {
    let mut runtime = extension_runtime();
    let mut pending = Vec::new();
    for index in 0..16 {
        let send = extension(&mut runtime, &format!("extension-{index}"));
        pending.push(deliver(&mut runtime, send));
        event(&mut runtime, "agent_start");
        event(&mut runtime, "agent_end");
        assert_terminal(&event(&mut runtime, "agent_settled"), TurnState::Completed);
    }
    let command = extension_command(&runtime, "after-capacity");
    assert!(runtime.command(command).is_err());
    assert!(runtime.discover_commands().is_err());
    for send in pending.iter().rev() {
        assert_no_terminal(&reply(&mut runtime, send, true));
    }
    let send = extension(&mut runtime, "after-capacity");
    let send = deliver(&mut runtime, send);
    assert_terminal(&reply(&mut runtime, &send, true), TurnState::Completed);
}

#[test]
fn cancelled_extension_before_delivery_can_roll_back_without_affecting_a_retry() {
    for resolution in [
        EgressResolution::QueueRejected,
        EgressResolution::WriteZeroBytes,
    ] {
        let mut runtime = extension_runtime();
        let send = extension(&mut runtime, "retry-client");
        assert!(
            runtime
                .ingest(&frame(json!({"type":"agent_start"})), &Mapper)
                .is_err()
        );
        let old_abort = runtime.command(AdapterCommand::Cancel).unwrap();
        reject_delivery(&mut runtime, send, resolution);
        let retry = extension(&mut runtime, "retry-client");
        let retry = deliver(&mut runtime, retry);
        let abort = runtime.command(AdapterCommand::Cancel).unwrap();
        let abort = deliver(&mut runtime, abort);
        reply(&mut runtime, &abort, true);
        reject_delivery(&mut runtime, old_abort, resolution);
        assert_terminal(&reply(&mut runtime, &retry, true), TurnState::Interrupted);
    }
}

#[test]
fn extension_failure_settles_once_and_honors_cancellation_at_delivery() {
    for cancel in [false, true] {
        let mut runtime = extension_runtime();
        let send = extension(&mut runtime, "extension-a");
        if cancel {
            let abort = runtime.command(AdapterCommand::Cancel).unwrap();
            deliver(&mut runtime, abort);
        }
        let send = deliver(&mut runtime, send);
        assert_terminal(
            &reply(&mut runtime, &send, false),
            if cancel {
                TurnState::Interrupted
            } else {
                TurnState::Failed
            },
        );
        assert_no_terminal(&reply(&mut runtime, &send, false));
        assert_no_terminal(&event(&mut runtime, "agent_settled"));
    }
}

#[test]
fn extension_unknown_delivery_fails_closed() {
    let mut runtime = extension_runtime();
    let send = extension(&mut runtime, "extension-a");
    let value = request(&send);
    let token = send.into_token();
    runtime
        .resolve_egress(token.clone(), EgressResolution::Queued)
        .unwrap();
    runtime
        .resolve_egress(token, EgressResolution::DeliveryUnknown)
        .unwrap();
    assert!(
        runtime
            .ingest(&response(&value, Value::Null), &Mapper)
            .is_err()
    );
    let command = extension_command(&runtime, "extension-b");
    assert!(runtime.command(command).is_err());
}
