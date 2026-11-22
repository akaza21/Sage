use super::*;

#[test]
fn the_first_idle_question_is_adopted_before_reading_the_stdout_behind_it() {
    let mut idle = activation_idle(false);
    let mut input = line(
        json!({"type":"extension_ui_request","method":"input","id":"first-idle","title":"Name?"}),
    );
    input.extend(line(
        json!({"type":"extension_ui_request","method":"notify","message":"Behind the question"}),
    ));
    idle.accept_idle_stdout_chunk(&input).unwrap();
    assert!(idle.parts.turn_started());
    assert!(!idle.idle_decoder.is_frame_aligned());
    assert!(idle.transport.0.is_empty());
    let (mut adapter, _, _, _) = idle.parts.into_parts();
    let card = adapter
        .adopt_turn(RequiredText::new("client", "idle-first").unwrap(), &Mapper)
        .unwrap();
    assert!(
        matches!(card.events(), [AdapterEvent::PermissionRequest { external_request_id, .. }] if external_request_id.as_str() == "first-idle")
    );
    let following = idle.idle_decoder.drain_pending().unwrap().unwrap();
    let following = adapter.ingest(&following, &Mapper).unwrap();
    assert!(matches!(following.events(), [AdapterEvent::Session { .. }]));
}

#[tokio::test]
async fn a_pi_timeout_cancels_through_the_store_without_another_wire_frame() {
    let fixture = Fixture::new();
    let mut adapter = ready_adapter();
    let clock = QuestionClock::new(&mut adapter.inner);
    let mut pump = fixture.pump(adapter);
    start(&mut pump).await;
    pump.push_stdout_chunk(&line(json!({"type":"extension_ui_request","method":"confirm","id":"deadline","title":"Continue?","message":"Apply the change?","timeout":20})), 10).await.unwrap();
    assert_eq!(pump.next_auto_resolution_deadline_millis(), Some(30));
    clock.advance_ms(19);
    assert!(!pump.auto_resolve_due(29).await.unwrap());
    clock.advance_ms(1);
    assert!(pump.auto_resolve_due(30).await.unwrap());
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert!(pump.settle_written_permission(31).await.unwrap());
    assert_eq!(
        transport.0,
        [json!({"type":"extension_ui_response","id":"deadline","cancelled":true})]
    );
    assert_eq!(pump.next_auto_resolution_deadline_millis(), None);
    let state: String = {
        let connection = fixture.database.connection();
        connection
            .query_row("SELECT state FROM server_requests", [], |row| row.get(0))
            .unwrap()
    };
    assert_eq!(state, "resolvedDeclined");
    settle(&mut pump).await;
    fixture.finish().await;
}
