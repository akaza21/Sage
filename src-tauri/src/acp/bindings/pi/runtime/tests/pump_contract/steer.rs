use super::*;

#[tokio::test]
async fn pi_pump_steer_is_answered_and_the_ended_turn_clears_pi_queue() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    let (_, request_id) = pump
        .queue_steer(
            Prompt::new("prompt", "Use the cache").unwrap(),
            RequiredText::new("client", "steer-1").unwrap(),
        )
        .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0[0]["type"], "steer");
    assert_eq!(transport.0[0]["message"], "Use the cache");
    pump.push_stdout_chunk(&reply(&transport.0[0], json!({"disposition":"queued"})), 10)
        .await
        .unwrap();
    assert_eq!(pump.take_steer_response(request_id), Some(true));

    pump.push_stdout_chunk(
        &line(json!({"type":"agent_end","messages":[{"role":"assistant","content":[],"stopReason":"aborted"}]})),
        20,
    )
    .await
    .unwrap();
    pump.push_stdout_chunk(&line(json!({"type":"agent_settled"})), 21)
        .await
        .unwrap();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(transport.0.len(), 2);
    assert_eq!(transport.0[1]["type"], "clear_queue");
    assert!(pump.completed_turn_ready());

    let mut idle = pump
        .try_into_completed_parts()
        .unwrap_or_else(|_| panic!("idle handoff"));
    idle.accept_idle_stdout_chunk(
        &reply(
            &transport.0[1],
            json!({"steering":["Use the cache"],"followUp":[]}),
        ),
        30,
        &mut Capture::default(),
    )
    .await
    .unwrap();
    fixture.finish().await;
}
