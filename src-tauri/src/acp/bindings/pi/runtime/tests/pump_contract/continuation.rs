use super::*;

#[tokio::test]
async fn pi_pump_compaction_continuation_keeps_ownership_across_ignored_session_notification() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(ready_adapter());
    start(&mut pump).await;
    let wire = include_str!("../../../../../../../testdata/acp/pi/compaction_continuation.jsonl");
    for (step, record) in wire.lines().enumerate() {
        let value: Value = serde_json::from_str(record).unwrap();
        let notification = value["type"] == "entry_appended";
        let settled = value["type"] == "agent_settled";
        fixture.observed.lock().unwrap().clear();
        pump.push_stdout_chunk(&line(value), step as i64 + 3)
            .await
            .unwrap();
        let events = fixture.observed.lock().unwrap().clone();
        if notification {
            assert!(matches!(events.as_slice(), [AdapterEvent::IgnoredLate]));
        } else {
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, AdapterEvent::IgnoredLate))
            );
        }
        assert_eq!(pump.completed_turn_ready(), settled);
        assert!(!pump.teardown_required());
    }
    {
        let connection = fixture.database.connection();
        let state: String = connection
            .query_row(
                "SELECT state FROM turns WHERE id='turn-pump-0'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "completed");
        let recovered: i64 = connection.query_row("SELECT count(*) FROM items WHERE turn_id='turn-pump-0' AND kind='text' AND lifecycle='completed' AND payload_json LIKE '%Recovered%'", [], |row| row.get(0)).unwrap();
        assert_eq!(recovered, 1);
    }
    fixture.finish().await;
}
