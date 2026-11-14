use super::dialogs::dialog;
use super::*;
use crate::acp::{
    bindings::adapter::{AdapterBatch, AdapterEgress, EgressToken},
    runtime::AdapterPumpError,
    store::load_chat_page,
    transport::OutboundFrame,
};
use sage_acp_protocol::lifecycle::TurnState;

#[tokio::test]
async fn ninth_unflushed_dialog_fails_closed_at_the_control_queue_cap() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for ordinal in 0..8 {
        pump.push_stdout_chunk(&line(dialog(&format!("pending-{ordinal}"), "input")), 3)
            .await
            .unwrap();
    }
    assert!(
        pump.push_stdout_chunk(&line(dialog("pending-8", "input")), 4)
            .await
            .is_err()
    );
    assert!(pump.teardown_required());
    assert_eq!(fixture.item_count(), 8);
    let mut transport = Capture::default();
    assert!(pump.flush_one(&mut transport).is_err());
    assert!(transport.0.is_empty());
    fixture.finish().await;
}

#[tokio::test]
async fn idle_dialogs_after_failed_and_interrupted_turns_do_not_mutate_history() {
    for (stop, state) in [
        ("error", TurnState::Failed),
        ("aborted", TurnState::Interrupted),
    ] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        pump.push_stdout_chunk(
            &line(json!({"type":"agent_end",
            "messages":[{"role":"assistant","stopReason":stop}]})),
            20,
        )
        .await
        .unwrap();
        pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 21)
            .await
            .unwrap();
        let before = {
            let connection = fixture.database.connection();
            let page =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            assert_eq!(page.turns.as_slice()[0].state, state);
            page
        };
        let mut idle = pump
            .try_into_completed_parts()
            .unwrap_or_else(|_| panic!("idle handoff"));
        let mut transport = Capture::default();
        idle.accept_idle_stdout_chunk(&line(dialog("after-terminal", "input")), 30, &mut transport)
            .await
            .unwrap();
        assert_eq!(
            transport.0,
            [json!({"type":"extension_ui_response",
            "id":"after-terminal","cancelled":true})]
        );
        {
            let connection = fixture.database.connection();
            let after =
                load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
            assert_eq!(before, after);
        }
        fixture.finish().await;
    }
}

#[test]
fn idle_dialog_ids_are_deduplicated_independently_per_session_runtime() {
    let mut left = activation_idle(false);
    let mut right = activation_idle(true);
    assert_ne!(
        left.parts.session().external_session_id,
        right.parts.session().external_session_id
    );
    let bytes = line(dialog("same-id", "input"));
    for _ in 0..2 {
        left.accept_idle_stdout_chunk(&bytes).unwrap();
        right.accept_idle_stdout_chunk(&bytes).unwrap();
    }
    let expected = [json!({"type":"extension_ui_response","id":"same-id","cancelled":true})];
    assert_eq!(left.transport.0, expected);
    assert_eq!(right.transport.0, expected);
}

#[test]
fn idle_dialogs_reject_empty_oversized_and_sensitive_ids_without_writing() {
    for id in [
        String::new(),
        "x".repeat(513),
        "sk-private-fixture".to_owned(),
    ] {
        let mut idle = activation_idle(false);
        assert!(
            idle.accept_idle_stdout_chunk(&line(dialog(&id, "input")))
                .is_err()
        );
        assert!(idle.transport.0.is_empty());
    }
}

#[test]
fn activation_idle_rejects_regular_egress_alone_or_mixed_with_cancellations_before_writing() {
    for mixed in [false, true] {
        for token in [
            EgressToken::request(1),
            EgressToken::protocol_notification(),
            EgressToken::permission_allow(ExternalId::new("request", "gate").unwrap()),
            EgressToken::mandatory_deny(ExternalId::new("request", "gate").unwrap(), false),
        ] {
            let mut idle = activation_idle(false);
            let mut egress = Vec::new();
            if mixed {
                egress.push(AdapterEgress::new(
                    OutboundFrame::new(b"{}".to_vec()).unwrap(),
                    EgressToken::permission_cancel(ExternalId::new("request", "dialog").unwrap()),
                ));
            }
            egress.push(AdapterEgress::new(
                OutboundFrame::new(b"{}".to_vec()).unwrap(),
                token,
            ));
            let batch = AdapterBatch::new(vec![AdapterEvent::IgnoredLate], egress).unwrap();
            let mut transport = Capture::default();
            assert!(matches!(
                idle.parts.accept_idle_batch(batch, &mut transport),
                Err(AdapterPumpError::Frame)
            ));
            assert!(transport.0.is_empty());
        }
    }
}
