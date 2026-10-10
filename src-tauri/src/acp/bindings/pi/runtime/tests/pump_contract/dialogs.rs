use super::*;
use crate::acp::lifecycle::ItemLifecycle;
use crate::acp::runtime::EgressTransport;

pub(super) fn dialog(id: &str, method: &str) -> Value {
    json!({"type":"extension_ui_request","id":id,"method":method,
        "title":"Choose a value","options":["Red","Blue"]})
}

#[tokio::test]
async fn repeated_dialog_id_is_ignored_before_and_after_cancellation_is_written() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    let bytes = line(dialog("duplicate", "input"));
    pump.push_stdout_chunk(&bytes, 3).await.unwrap();
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(&bytes, 4).await.unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    pump.push_stdout_chunk(&bytes, 5).await.unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(
        transport.0,
        [json!({"type":"extension_ui_response",
        "id":"duplicate","cancelled":true})]
    );
    assert_eq!(fixture.item_count(), 1);
    assert!(
        fixture
            .observed
            .lock()
            .unwrap()
            .iter()
            .all(|event| matches!(event, AdapterEvent::IgnoredDuplicate))
    );
    assert!(!pump.teardown_required());
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn dialog_notice_cap_is_eight_per_turn_but_every_unique_dialog_is_answered() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for ordinal in 0..9 {
        let id = format!("capped-{ordinal}");
        pump.push_stdout_chunk(&line(dialog(&id, "editor")), 3 + ordinal)
            .await
            .unwrap();
        let mut transport = Capture::default();
        pump.flush_one(&mut transport).unwrap();
        assert_eq!(
            transport.0,
            [json!({"type":"extension_ui_response",
            "id":id,"cancelled":true})]
        );
    }
    assert_eq!(fixture.item_count(), 8);
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    pump.push_stdout_chunk(&line(dialog("next-turn", "input")), 30)
        .await
        .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(
        transport.0,
        [json!({"type":"extension_ui_response",
        "id":"next-turn","cancelled":true})]
    );
    assert_eq!(fixture.item_count(), 9);
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn dialog_deduplication_retains_only_the_sixteen_most_recent_written_ids() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for ordinal in 0..17 {
        pump.push_stdout_chunk(&line(dialog(&format!("recent-{ordinal}"), "input")), 3)
            .await
            .unwrap();
        pump.flush_one(&mut Capture::default()).unwrap();
    }
    let mut transport = Capture::default();
    pump.push_stdout_chunk(&line(dialog("recent-1", "input")), 4)
        .await
        .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert!(transport.0.is_empty());
    pump.push_stdout_chunk(&line(dialog("recent-0", "input")), 5)
        .await
        .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(
        transport.0,
        [json!({"type":"extension_ui_response",
        "id":"recent-0","cancelled":true})]
    );
    assert_eq!(fixture.item_count(), 8);
    settle(&mut pump).await;
    fixture.finish().await;
}

#[test]
fn idle_dialogs_are_cancelled_before_the_first_prompt_and_after_session_reset() {
    for reset in [false, true] {
        let mut idle = activation_idle(reset);
        for method in ["input", "editor", "select", "confirm"] {
            idle.accept_idle_stdout_chunk(&line(dialog(method, method)))
                .unwrap();
        }
        assert_eq!(
            idle.transport.0,
            ["input", "editor", "select", "confirm"]
                .map(|id| json!({"type":"extension_ui_response","id":id,"cancelled":true}))
        );
    }
}

#[tokio::test]
async fn idle_dialogs_after_settlement_are_answered_without_adding_turn_items() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    settle(&mut pump).await;
    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    let mut transport = Capture::default();
    for method in ["input", "editor", "select", "confirm"] {
        let bytes = line(dialog(method, method));
        idle.accept_idle_stdout_chunk(&bytes, 30, &mut transport)
            .await
            .unwrap();
        idle.accept_idle_stdout_chunk(&bytes, 31, &mut transport)
            .await
            .unwrap();
    }
    assert_eq!(
        transport.0,
        ["input", "editor", "select", "confirm"]
            .map(|id| json!({"type":"extension_ui_response","id":id,"cancelled":true}))
    );
    assert_eq!(fixture.item_count(), 0);
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    settle(&mut pump).await;
    fixture.finish().await;
}

#[test]
fn resetting_the_session_allows_an_idle_dialog_id_to_be_reused() {
    let mut idle = activation_idle(false);
    let bytes = line(dialog("reused", "input"));
    idle.accept_idle_stdout_chunk(&bytes).unwrap();
    idle.accept_idle_stdout_chunk(&bytes).unwrap();
    assert_eq!(idle.transport.0.len(), 1);
    let (adapter, _, _, _) = idle.parts.into_parts();
    let mut reset = activation_with(adapter, true);
    reset.accept_idle_stdout_chunk(&bytes).unwrap();
    assert_eq!(
        reset.transport.0,
        [json!({"type":"extension_ui_response",
        "id":"reused","cancelled":true})]
    );
}

struct FailedIdleWrite(bool);
impl EgressTransport for FailedIdleWrite {
    fn write(
        &mut self,
        _: &crate::acp::transport::OutboundFrame,
    ) -> crate::acp::transport::FrameWriteOutcome {
        if self.0 {
            crate::acp::transport::FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 }
        } else {
            crate::acp::transport::FrameWriteOutcome::ZeroByteFailure
        }
    }
}

#[tokio::test]
async fn idle_dialog_cancellation_write_failures_require_teardown_before_and_between_turns() {
    for unknown in [false, true] {
        let mut activation = activation_idle(false);
        let batch = activation
            .parts
            .ingest_idle_frame(&frame(dialog("idle", "input")))
            .unwrap();
        assert!(matches!(
            activation
                .parts
                .accept_idle_batch(batch, &mut FailedIdleWrite(unknown)),
            Err(crate::acp::runtime::AdapterPumpError::TeardownRequired)
        ));
        assert!(
            activation
                .accept_idle_stdout_chunk(&line(dialog("after-failure", "input")))
                .is_err()
        );
        assert!(activation.transport.0.is_empty());
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        settle(&mut pump).await;
        let mut idle = pump
            .try_into_completed_parts()
            .unwrap_or_else(|_| panic!("idle handoff"));
        assert!(matches!(
            idle.accept_idle_stdout_chunk(
                &line(dialog("idle", "input")),
                30,
                &mut FailedIdleWrite(unknown)
            )
            .await,
            Err(crate::acp::runtime::AdapterPumpError::TeardownRequired)
        ));
        assert_eq!(fixture.item_count(), 0);
        let mut transport = Capture::default();
        assert!(
            idle.accept_idle_stdout_chunk(
                &line(dialog("after-failure", "input")),
                31,
                &mut transport
            )
            .await
            .is_err()
        );
        assert!(transport.0.is_empty());
        fixture.finish().await;
    }
}

#[tokio::test]
async fn cancelled_dialog_is_a_terminal_item_with_human_readable_copy() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(&line(dialog("notice", "input")), 3)
        .await
        .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.as_slice(), [AdapterEvent::Item {
        lifecycle: ItemLifecycle::Cancelled,
        payload: NormalizedPayload::Unsupported { external_kind, summary }, ..
    }] if external_kind.as_str() == "pi_dialog_cancelled"
        && summary.as_str() == "Pi extension dialog was cancelled: this dialog is not supported"));
    pump.flush_one(&mut Capture::default()).unwrap();
    settle(&mut pump).await;
    fixture.finish().await;
}
