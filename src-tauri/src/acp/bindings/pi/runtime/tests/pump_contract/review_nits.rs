use super::*;

fn unsupported_dialog(method: &str) -> Value {
    json!({"type":"extension_ui_request","id":format!("dialog-{method}"),"method":method,
        "title":"Choose a value","options":["Red","Blue"]})
}

#[tokio::test]
async fn pi_pump_unsupported_dialogs_cancel_and_settle() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for method in ["input", "editor", "select"] {
        fixture.observed.lock().unwrap().clear();
        pump.push_stdout_chunk(&line(unsupported_dialog(method)), 5)
            .await
            .unwrap();
        let mut transport = Capture::default();
        pump.flush_one(&mut transport).unwrap();
        assert_eq!(
            transport.0,
            vec![json!({"type":"extension_ui_response",
            "id":format!("dialog-{method}"),"cancelled":true})]
        );
        let events = fixture.observed.lock().unwrap().clone();
        assert!(matches!(events.as_slice(), [AdapterEvent::Unsupported(
            NormalizedPayload::Unsupported { external_kind, summary }
        )] if external_kind.as_str() == "pi_dialog_cancelled"
            && summary.as_str() == "Pi extension dialog was cancelled: this dialog is not supported"));
    }
    assert_eq!(fixture.item_count(), 3);
    settle(&mut pump).await;
    {
        let connection = fixture.database.connection();
        let state: String = connection
            .query_row("SELECT state FROM turns WHERE ordinal=0", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(
            state, "completed",
            "cancelling a dialog must not cancel its turn"
        );
    }
    assert!(!pump.teardown_required());
    fixture.finish().await;
}

struct FailedWrite(bool);
impl crate::acp::runtime::EgressTransport for FailedWrite {
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
async fn pi_pump_dialog_cancel_delivery_failure_requires_teardown() {
    for unknown in [false, true] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(ready_adapter());
        start(&mut pump).await;
        pump.push_stdout_chunk(&line(unsupported_dialog("input")), 5)
            .await
            .unwrap();
        let _ = pump.flush_one(&mut FailedWrite(unknown));
        assert!(pump.teardown_required(), "dialog cancellation is mandatory");
        fixture.finish().await;
    }
}

fn message_fallback(delta: bool) -> Value {
    if delta {
        json!({"type":"message_update","assistantMessageEvent":{"type":"future_delta","delta":"unknown"}})
    } else {
        json!({"type":"message_update","assistantMessageEvent":{"type":"future_start"}})
    }
}

#[tokio::test]
async fn pi_pump_message_fallbacks_share_unknown_kind_budget() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for _ in 0..200 {
        for delta in [false, true] {
            pump.push_stdout_chunk(&line(message_fallback(delta)), 5)
                .await
                .unwrap();
        }
    }
    assert_eq!(fixture.item_count(), 2);
    fixture.observed.lock().unwrap().clear();
    for kind in 0..31 {
        pump.push_stdout_chunk(&line(json!({"type":format!("future-{kind}")})), 6)
            .await
            .unwrap();
    }
    assert_eq!(fixture.item_count(), 33);
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.last(), Some(AdapterEvent::Unsupported(
        NormalizedPayload::Unsupported { external_kind, .. }
    )) if external_kind.as_str() == "pi_unknown_event_limit"));
    fixture.observed.lock().unwrap().clear();
    for delta in [false, true] {
        pump.push_stdout_chunk(&line(message_fallback(delta)), 7)
            .await
            .unwrap();
    }
    for kind in 31..35 {
        pump.push_stdout_chunk(&line(json!({"type":format!("future-{kind}")})), 7)
            .await
            .unwrap();
    }
    let events = fixture.observed.lock().unwrap().clone();
    assert!(
        events
            .iter()
            .all(|event| matches!(event, AdapterEvent::Absorbed))
    );
    assert_eq!(fixture.item_count(), 33);
    settle(&mut pump).await;
    let idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    fixture.add_turn(1);
    let mut pump = idle
        .bind_turn(fixture.intent(1).active_turn)
        .unwrap_or_else(|_| panic!("second turn"));
    start_turn(&mut pump, 1).await;
    for delta in [false, true] {
        pump.push_stdout_chunk(&line(message_fallback(delta)), 8)
            .await
            .unwrap();
    }
    assert_eq!(fixture.item_count(), 35);
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_message_fallbacks_obey_full_unknown_kind_budget() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    for kind in 0..32 {
        pump.push_stdout_chunk(&line(json!({"type":format!("future-{kind}")})), 5)
            .await
            .unwrap();
    }
    fixture.observed.lock().unwrap().clear();
    for delta in [false, true] {
        pump.push_stdout_chunk(&line(message_fallback(delta)), 6)
            .await
            .unwrap();
    }
    let events = fixture.observed.lock().unwrap().clone();
    assert!(matches!(events.as_slice(), [AdapterEvent::Unsupported(
        NormalizedPayload::Unsupported { external_kind, .. }
    ), AdapterEvent::Absorbed] if external_kind.as_str() == "pi_unknown_event_limit"));
    assert_eq!(fixture.item_count(), 33);
    settle(&mut pump).await;
    fixture.finish().await;
}

#[tokio::test]
async fn pi_pump_image_fallback_reports_pi_admission_gap() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    pump.push_stdout_chunk(&line(tool_start("image-read")), 5)
        .await
        .unwrap();
    fixture.observed.lock().unwrap().clear();
    pump.push_stdout_chunk(
        &line(tool_end(
            "image-read",
            json!([
                {"type":"image","mimeType":"image/png","data":"not-base64"}
            ]),
        )),
        6,
    )
    .await
    .unwrap();
    let events = fixture.observed.lock().unwrap().clone();
    assert!(events.iter().any(|event| matches!(event, AdapterEvent::Item {
        payload: NormalizedPayload::Unsupported { external_kind, summary }, ..
    } if external_kind.as_str() == "pi_image" && summary.as_str() == "Pi image output is not yet admitted")));
    assert!(
        !serde_json::to_string(&events)
            .unwrap()
            .contains("not-base64")
    );
    settle(&mut pump).await;
    fixture.finish().await;
}
