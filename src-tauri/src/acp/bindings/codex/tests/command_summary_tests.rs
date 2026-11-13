use super::*;
use crate::acp::{intent::ToolCategory, lifecycle::ItemLifecycle};

#[test]
fn unresolved_read_summaries_preserve_completion_at_the_byte_limit() {
    for (padding, suffix, retained_suffix) in [
        (65_524, "", ""),
        (65_525, "", ""),
        (65_526, "", ""),
        (65_523, "é", "é"),
        (65_524, "é", ""),
    ] {
        let mut adapter = adapter();
        ready(&mut adapter);
        arm_turn(&mut adapter, 2, "client-read-summary");
        adapter
            .ingest(
                br#"{"method":"turn/started","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"inProgress"}}}"#,
                &Mapper,
            )
            .unwrap();
        let started = json!({"method":"item/started","params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"commandExecution","id":"read-1","status":"inProgress",
                "command":format!("cat C:\\foreign\\file {}{suffix}", "x".repeat(padding)),
                "commandActions":[{"type":"read","name":"file","path":"C:\\foreign\\file"}]
            }
        }});
        let completed = json!({"method":"item/completed","params":{
            "threadId":"thread-1","turnId":"turn-1","item":{
                "type":"commandExecution","id":"read-1","status":"completed"
            }
        }});
        for (frame, expected_lifecycle) in [
            (started, ItemLifecycle::Started),
            (completed, ItemLifecycle::Completed),
        ] {
            let IngressDisposition::Events(events) = adapter
                .ingest(&serde_json::to_vec(&frame).unwrap(), &Mapper)
                .expect("a long display summary must not cause teardown")
            else {
                panic!("expected a read event");
            };
            let [
                IncomingEvent::Item {
                    lifecycle,
                    payload:
                        NormalizedPayload::ToolCall {
                            category: ToolCategory::Read,
                            input_summary: Some(summary),
                            ..
                        },
                    ..
                },
            ] = events.as_slice()
            else {
                panic!("expected an unresolved read with its command summary");
            };
            assert_eq!(*lifecycle, expected_lifecycle);
            assert_eq!(
                summary.as_str(),
                format!(
                    "cat [path] {}{retained_suffix}",
                    "x".repeat(padding.min(65_525))
                )
            );
        }
        assert!(matches!(
            adapter.ingest(
                br#"{"method":"turn/completed","params":{"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}}"#,
                &Mapper,
            ).unwrap(),
            IngressDisposition::Events(events) if matches!(
                events.as_slice(), [IncomingEvent::Turn { state: TurnState::Completed, .. }]
            )
        ));
    }
}
