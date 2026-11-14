use super::*;

const STEER: &str = "Synthetic steer: use the other example.";

fn journey() -> Vec<Value> {
    let requests = [
        json!({"id":1,"method":"initialize"}),
        json!({"id":2,"method":"thread/start"}),
        json!({"id":3,"method":"thread/goal/get"}),
        json!({"id":4,"method":"turn/start"}),
        json!({"id":5,"method":"turn/steer"}),
        json!({"id":6,"method":"thread/unsubscribe"}),
    ]
    .iter()
    .map(|request| format!("{request}\n"))
    .collect::<String>();
    crate::acp::fixture::replay_native("codex-steered-message", &requests)
}

fn ingest(runtime: &mut CodexRuntime, value: &Value) -> Vec<AdapterEvent> {
    runtime
        .ingest(&frame(value.clone()), &Mapper)
        .unwrap_or_else(|error| panic!("refused {value}: {error:?}"))
        .events()
        .to_vec()
}

fn answer(
    runtime: &mut CodexRuntime,
    egress: AdapterEgress,
    frames: &[Value],
    journey_id: u64,
) -> Vec<AdapterEvent> {
    let mut reply = frames
        .iter()
        .find(|frame| frame["id"] == journey_id)
        .unwrap()
        .clone();
    reply["id"] = request_id(&egress).into();
    runtime
        .resolve_egress(egress.into_token(), EgressResolution::Queued)
        .unwrap();
    ingest(runtime, &reply)
}

fn texts(events: &[AdapterEvent]) -> Vec<&str> {
    events
        .iter()
        .filter_map(|event| match event {
            AdapterEvent::Item {
                payload: NormalizedPayload::Text { text },
                ..
            } => Some(text.as_str()),
            _ => None,
        })
        .collect()
}

#[test]
fn steered_message_journey_accepts_the_steer_and_drops_the_codex_echo() {
    let frames = journey();
    let mut runtime = fixture_runtime();
    initialize(&mut runtime);
    let start = runtime
        .command(AdapterCommand::NewSession {
            native_root: r"C:\repo".into(),
            model: None,
        })
        .unwrap();
    answer(&mut runtime, start, &frames, 2);
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Work on the example").unwrap(),
            client_message_id: RequiredText::new("client", "client-1").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    answer(&mut runtime, prompt, &frames, 4);
    let notifications: Vec<_> = frames
        .iter()
        .filter(|frame| frame["id"].is_null())
        .collect();
    let [
        started,
        before,
        echo_started,
        echo_completed,
        after,
        completed,
    ] = notifications[..]
    else {
        panic!("the journey's notifications: {notifications:?}")
    };
    ingest(&mut runtime, started);
    assert_eq!(
        texts(&ingest(&mut runtime, before)),
        ["Working on the example; send a steer."]
    );
    let steer = runtime
        .command(AdapterCommand::Steer {
            prompt: Prompt::new("prompt", STEER).unwrap(),
            client_message_id: RequiredText::new("client", "steer-1").unwrap(),
        })
        .unwrap();
    let request_id = request_id(&steer);
    assert_eq!(
        answer(&mut runtime, steer, &frames, 5),
        [AdapterEvent::SteerResponse {
            request_id,
            accepted: true
        }]
    );
    for echo in [echo_started, echo_completed] {
        assert_eq!(echo["params"]["item"]["type"], "userMessage");
        assert!(ingest(&mut runtime, echo).is_empty());
    }
    assert_eq!(
        texts(&ingest(&mut runtime, after)),
        ["Switched to the other example after your steer."]
    );
    ingest(&mut runtime, completed);
}
