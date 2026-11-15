use super::*;

#[test]
fn an_elicitation_for_a_foreign_thread_is_refused_and_the_session_stays_alive() {
    let mut runtime = fixture_runtime();
    ready(&mut runtime);
    let batch = runtime
        .ingest(
            &frame(
                json!({"id":4711,"method":"mcpServer/elicitation/request","params":{
                    "threadId":"thread-foreign","turnId":null,"serverName":"synthetic",
                    "mode":"form","message":"Pick one","_meta":null,
                    "requestedSchema":{"type":"object","properties":{}}
                }}),
            ),
            &Mapper,
        )
        .expect("a foreign elicitation must not tear the runtime down");
    let [reply] = batch.egress() else {
        panic!("the elicitation must be answered once");
    };
    let reply: Value = serde_json::from_slice(reply.frame().payload()).unwrap();
    assert_eq!(reply["id"], 4711);
    assert_eq!(reply["result"], json!({"action":"cancel"}));
    assert!(reply.get("error").is_none());
    assert!(
        batch
            .events()
            .iter()
            .all(|event| !matches!(event, AdapterEvent::Item { .. }))
    );
    running_turn(&mut runtime);
    let completed = runtime
        .ingest(
            &frame(json!({"method":"turn/completed","params":{
                "threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}
            }})),
            &Mapper,
        )
        .unwrap();
    assert!(matches!(
        completed.events(),
        [AdapterEvent::Turn { state, .. }] if state.is_terminal()
    ));
}
