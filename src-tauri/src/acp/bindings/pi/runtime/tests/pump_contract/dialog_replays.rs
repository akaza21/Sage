use super::*;
use crate::acp::intent::PermissionOptionKind;
use crate::acp::runtime::permissions::PermissionDecision;

fn approval_dialog(id: &str, method: &str) -> Value {
    json!({"type":"extension_ui_request","id":id,"method":method,
        "title":"Allow tool?","options":["Deny","Allow once"]})
}

#[tokio::test]
async fn idle_cancelled_approval_dialogs_cannot_reopen_after_agent_start() {
    for method in ["confirm", "select"] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        settle(&mut pump).await;
        let mut idle = pump
            .try_into_completed_parts()
            .unwrap_or_else(|_| panic!("idle handoff"));
        let bytes = line(approval_dialog("idle-cancelled", method));
        let mut transport = Capture::default();
        idle.accept_idle_stdout_chunk(&bytes, 30, &mut transport)
            .await
            .unwrap();
        assert_eq!(
            transport.0,
            [json!({"type":"extension_ui_response","id":"idle-cancelled","cancelled":true})]
        );
        fixture.add_turn(1);
        let mut pump = idle
            .bind_turn(fixture.intent(1).active_turn)
            .unwrap_or_else(|_| panic!("next turn"));
        start_turn(&mut pump, 1).await;
        fixture.observed.lock().unwrap().clear();
        pump.push_stdout_chunk(&bytes, 32).await.unwrap();
        assert!(matches!(
            fixture.observed.lock().unwrap().as_slice(),
            [AdapterEvent::IgnoredDuplicate]
        ));
        let mut replay_transport = Capture::default();
        pump.flush_one(&mut replay_transport).unwrap();
        assert!(replay_transport.0.is_empty());
        assert_eq!(fixture.item_count(), 0);
        settle(&mut pump).await;
        assert!(!pump.teardown_required());
        assert!(pump.try_into_completed_parts().is_ok());
        fixture.finish().await;
    }
}

#[tokio::test]
async fn approval_dialog_cancel_pending_before_agent_start_stays_deduplicated() {
    for method in ["confirm", "select"] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        pump.queue_command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Inspect").unwrap(),
            client_message_id: RequiredText::new("client", "turn-pump-0").unwrap(),
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .unwrap();
        let mut transport = Capture::default();
        pump.flush_one(&mut transport).unwrap();
        pump.push_stdout_chunk(&reply(&transport.0[0], Value::Null), 1)
            .await
            .unwrap();
        let bytes = line(approval_dialog("pending-cancel", method));
        pump.push_stdout_chunk(&bytes, 2).await.unwrap();
        assert_eq!(fixture.item_count(), 1);
        pump.push_stdout_chunk(&line(json!({"type":"agent_start"})), 3)
            .await
            .unwrap();
        for written in [false, true] {
            fixture.observed.lock().unwrap().clear();
            pump.push_stdout_chunk(&bytes, 4).await.unwrap();
            assert!(matches!(
                fixture.observed.lock().unwrap().as_slice(),
                [AdapterEvent::IgnoredDuplicate]
            ));
            let mut cancellation = Capture::default();
            pump.flush_one(&mut cancellation).unwrap();
            if written {
                assert!(cancellation.0.is_empty());
            } else {
                assert_eq!(
                    cancellation.0,
                    [
                        json!({"type":"extension_ui_response","id":"pending-cancel","cancelled":true})
                    ]
                );
            }
        }
        assert_eq!(fixture.item_count(), 1);
        settle(&mut pump).await;
        assert!(!pump.teardown_required());
        fixture.finish().await;
    }
}

#[test]
fn cancelled_dialog_replay_does_not_replace_a_genuine_pending_approval() {
    for method in ["confirm", "select"] {
        let mut runtime = fixture_runtime(92);
        ready(&mut runtime);
        let batch = runtime
            .ingest(&frame(approval_dialog("cancelled", method)), &Mapper)
            .unwrap();
        let (_, mut egress) = batch.into_parts();
        assert_eq!(egress.len(), 1);
        let cancellation = egress.remove(0);
        runtime
            .commit_mandatory_egress(cancellation.token())
            .unwrap();
        deliver(&mut runtime, cancellation);
        running(&mut runtime);
        let batch = runtime
            .ingest(&frame(approval_dialog("fresh", method)), &Mapper)
            .unwrap();
        // The card offers exactly the answers the runtime can deliver below.
        assert!(matches!(batch.events(), [AdapterEvent::PermissionRequest {
            external_request_id,
            payload: NormalizedPayload::PermissionRequest {
                offered_option_kinds: Some(kinds), ..
            },
            ..
        }] if external_request_id.as_str() == "fresh"
            && kinds.as_slice() == [PermissionOptionKind::AllowOnce, PermissionOptionKind::RejectOnce]));
        let batch = runtime
            .ingest(&frame(approval_dialog("cancelled", method)), &Mapper)
            .unwrap();
        assert!(matches!(batch.events(), [AdapterEvent::IgnoredDuplicate]));
        assert!(batch.egress().is_empty());
        assert!(matches!(
            runtime.command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse("cancelled").unwrap(),
                decision: PermissionDecision::AllowOnce,
            }),
            Err(AdapterRuntimeError::LaterPermissionMiss)
        ));
        let reply = runtime
            .command(AdapterCommand::ResolvePermission {
                request_id: SageServerRequestId::parse("fresh").unwrap(),
                decision: PermissionDecision::AllowOnce,
            })
            .unwrap();
        assert_eq!(
            request(&reply),
            if method == "select" {
                json!({"type":"extension_ui_response","id":"fresh","value":"Allow once"})
            } else {
                json!({"type":"extension_ui_response","id":"fresh","confirmed":true})
            }
        );
    }
}
