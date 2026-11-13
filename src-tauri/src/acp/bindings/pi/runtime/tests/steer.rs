use super::*;

fn steer(runtime: &mut PiRuntime, text: &str) -> Result<AdapterEgress, AdapterRuntimeError> {
    runtime.command(AdapterCommand::Steer {
        prompt: Prompt::new("prompt", text).unwrap(),
        client_message_id: RequiredText::new("client", "steer").unwrap(),
    })
}

fn reply(request: &Value, success: bool) -> Frame {
    frame(json!({
        "type":"response",
        "id":request["id"],
        "command":"steer",
        "success":success,
        "data":{"disposition":"queued"}
    }))
}

#[test]
fn steer_sends_pi_steer_and_reports_each_answer_to_its_request() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    assert!(matches!(
        steer(&mut runtime, "too early"),
        Err(AdapterRuntimeError::Operation)
    ));
    running(&mut runtime);
    for accepted in [true, false] {
        let egress = steer(&mut runtime, "use the cache").unwrap();
        let request_id = egress.token().request_id().unwrap();
        let value = deliver(&mut runtime, egress);
        assert_eq!(value["type"], "steer");
        assert_eq!(value["message"], "use the cache");
        assert!(matches!(
            steer(&mut runtime, "again"),
            Err(AdapterRuntimeError::Operation)
        ));
        let batch = runtime.ingest(&reply(&value, accepted), &Mapper).unwrap();
        assert_eq!(
            batch.events(),
            [AdapterEvent::SteerResponse {
                request_id,
                accepted
            }]
        );
    }
}

#[test]
fn steer_answered_after_the_turn_ends_is_late_and_frees_the_next_turn() {
    let mut runtime = fixture_runtime(1);
    ready(&mut runtime);
    running(&mut runtime);
    let egress = steer(&mut runtime, "late").unwrap();
    let value = deliver(&mut runtime, egress);
    for event in [
        json!({"type":"agent_end","messages":[]}),
        json!({"type":"agent_settled"}),
    ] {
        runtime.ingest(&frame(event), &Mapper).unwrap();
    }
    let batch = runtime.ingest(&reply(&value, true), &Mapper).unwrap();
    assert_eq!(batch.events(), [AdapterEvent::IgnoredLate]);
    running(&mut runtime);
    assert!(steer(&mut runtime, "next turn").is_ok());
}
