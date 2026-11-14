use super::*;

#[test]
fn an_unwritten_cancel_does_not_hide_a_prompt_failure() {
    for resolution in [EgressResolution::Queued, EgressResolution::WriteZeroBytes] {
        let mut runtime = ClaudeRuntime::fixture();
        ready(&mut runtime);
        let (id, token) = prompt(&mut runtime);
        runtime
            .resolve_egress(token, EgressResolution::Written)
            .unwrap();
        let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
        runtime
            .resolve_egress(cancel.into_token(), resolution)
            .unwrap();
        let batch = runtime
            .ingest(
                &frame(json!({"id":id,"error":{"code":-32603,"message":"Prompt failed"}})),
                &Mapper,
            )
            .unwrap();
        assert!(batch.events().iter().any(|event| matches!(
            event,
            SharedEvent::Turn {
                state: TurnState::Failed,
                ..
            }
        )));
    }
}

#[test]
fn prompt_error_after_written_user_cancel_is_cancelled_and_next_turn_can_run() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();
    let batch = runtime
        .ingest(
            &frame(json!({"id":id,"error":{"code":-32603,"message":"Request aborted"}})),
            &Mapper,
        )
        .unwrap();
    assert!(batch.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Cancelled,
            ..
        }
    )));
    assert!(!batch.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Failed,
            ..
        }
    )));
    let (next_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let completed = runtime
        .ingest(
            &frame(json!({"id":next_id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
    assert!(completed.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Completed,
            ..
        }
    )));
}

#[test]
fn duplicate_aborted_response_after_stop_does_not_fail_the_retry() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let cancel = runtime.command(AdapterCommand::Cancel).unwrap();
    runtime
        .resolve_egress(cancel.into_token(), EgressResolution::Written)
        .unwrap();
    let aborted = json!({"id":id,"error":{"code":-32603,"message":"Request aborted"}});
    runtime.ingest(&frame(aborted.clone()), &Mapper).unwrap();
    let duplicate = runtime
        .ingest(&frame(aborted.clone()), &Mapper)
        .expect("identical cancelled response must be idempotent");
    assert!(matches!(
        duplicate.events(),
        [SharedEvent::IgnoredDuplicate]
    ));
    let (next_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    let duplicate = runtime
        .ingest(&frame(aborted), &Mapper)
        .expect("old cancelled response must not kill the next turn");
    assert!(matches!(
        duplicate.events(),
        [SharedEvent::IgnoredDuplicate]
    ));
    assert!(
        matches!(
            runtime.ingest(
                &frame(json!({"id":id,"result":{"stopReason":"end_turn"}})),
                &Mapper
            ),
            Err(AdapterRuntimeError::Frame)
        ),
        "a conflicting success is still a protocol violation"
    );
    let completed = runtime
        .ingest(
            &frame(json!({"id":next_id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
    assert!(completed.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Completed,
            ..
        }
    )));
}

#[test]
fn joined_browser_permissions_reuse_capacity_after_allowed_replies() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    let (prompt_id, token) = prompt(&mut runtime);
    runtime
        .resolve_egress(token, EgressResolution::Written)
        .unwrap();
    for index in 0..24 {
        let tool = format!("browser-tool-{index}");
        runtime
            .ingest(
                &frame(json!({"method":"session/update","params":{
                    "sessionId":"claude-session-1","update":{
                        "sessionUpdate":"tool_call","toolCallId":tool,
                        "kind":"fetch","status":"pending","title":"Browser action"
                    }
                }})),
                &Mapper,
            )
            .unwrap();
        let batch = runtime
            .ingest(
                &frame(json!({
                    "id":index,"method":"session/request_permission","params":{
                        "sessionId":"claude-session-1","toolCall":{
                            "toolCallId":tool,"kind":"fetch","status":"pending",
                            "rawInput":{"scope":{"tabId":2}}
                        },
                        "options":[
                            {"kind":"allow_once","optionId":"allow","name":"Yes"},
                            {"kind":"allow_always","optionId":"always","name":"Always allow"},
                            {"kind":"reject_once","optionId":"deny","name":"No"}
                        ]
                    }
                })),
                &Mapper,
            )
            .expect("joined-tab request must not kill the turn");
        let request = batch
            .events()
            .iter()
            .find_map(|event| match event {
                SharedEvent::PermissionRequest {
                    external_request_id,
                    ..
                } => Some(SageServerRequestId::parse(external_request_id.as_str()).unwrap()),
                _ => None,
            })
            .expect("permission admitted");
        let reply = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: request,
                decision: if index % 2 == 0 {
                    crate::acp_protocol::permissions::PermissionDecision::AllowOnce
                } else {
                    crate::acp_protocol::permissions::PermissionDecision::AllowAlways
                },
            })
            .unwrap();
        let token = reply.into_token();
        runtime
            .resolve_egress(token.clone(), EgressResolution::Queued)
            .unwrap();
        runtime
            .resolve_egress(token.clone(), EgressResolution::Written)
            .unwrap();
        runtime
            .resolve_egress(token, EgressResolution::Written)
            .unwrap();
        let completed = runtime
            .ingest(
                &frame(json!({"method":"session/update","params":{
                    "sessionId":"claude-session-1","update":{
                        "sessionUpdate":"tool_call_update","toolCallId":tool,"status":"completed"
                    }
                }})),
                &Mapper,
            )
            .unwrap();
        assert!(completed.events().iter().any(|event| matches!(
            event,
            SharedEvent::Item {
                tool_state: Some(ToolState::Completed),
                ..
            }
        )));
    }
    let terminal = runtime
        .ingest(
            &frame(json!({"id":prompt_id,"result":{"stopReason":"end_turn"}})),
            &Mapper,
        )
        .unwrap();
    assert!(terminal.events().iter().any(|event| matches!(
        event,
        SharedEvent::Turn {
            state: TurnState::Completed,
            ..
        }
    )));
}
