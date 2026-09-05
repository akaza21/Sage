use super::*;

#[test]
fn cancelled_request_and_item_commit_atomically_and_duplicate_idempotently() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("cancel permission")).unwrap();
    connection.execute(
        "INSERT INTO items VALUES('permission-cancel','project-one','chat-one','session-one',?1,1,1,'primary',NULL,'permissionRequest','started','waitingForApproval','external-cancel','{}',2,'digest-cancel',0,0,0,NULL)",
        [prepared.turn_id.as_str()],
    ).unwrap();
    connection.execute(
        "INSERT INTO server_requests VALUES('request-cancel','project-one','chat-one','session-one',?1,1,'external-cancel','digest-cancel','{}','received',0,0,NULL)",
        [prepared.turn_id.as_str()],
    ).unwrap();
    let command = RequestStateCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        turn_id: prepared.turn_id,
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        request_id: SageServerRequestId::parse("request-cancel").unwrap(),
        state: ServerRequestState::Cancelled,
        now_millis: 30,
    };

    let applied = apply_request_state(&mut connection, command.clone()).unwrap();
    assert_eq!(applied.disposition, MutationDisposition::Applied);
    let row: (String, Option<i64>, String, String) = connection.query_row(
        "SELECT r.state,r.resolved_at,i.lifecycle,i.tool_state FROM server_requests r JOIN items i ON i.external_item_id=r.external_request_id WHERE r.id='request-cancel'",
        [],
        |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
    ).unwrap();
    assert_eq!(
        row,
        (
            "cancelled".into(),
            Some(30),
            "completed".into(),
            "cancelled".into()
        )
    );

    let duplicate = apply_request_state(&mut connection, command).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
}
