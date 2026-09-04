use super::*;

#[test]
fn request_identity_mismatch_never_becomes_a_same_content_duplicate() {
    let mut connection = setup();
    connection
        .execute(
            "INSERT INTO server_requests VALUES('request-one','project-one','chat-one','session-one','turn-one',1,'external-one','digest-one','{}','asked',4,4,NULL)",
            [],
        )
        .unwrap();
    let exact = RequestStateCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: SageAgentSessionId::parse("session-one").unwrap(),
        turn_id: SageTurnId::parse("turn-one").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        request_id: SageServerRequestId::parse("request-one").unwrap(),
        state: ServerRequestState::Asked,
        now_millis: 5,
    };
    let before = request_snapshot(&connection);

    for field in [
        "projectId",
        "chatId",
        "sessionId",
        "turnId",
        "runtimeEpoch",
        "requestId",
    ] {
        let mut forged = exact.clone();
        match field {
            "projectId" => forged.project_id = SageProjectId::parse("project-forged").unwrap(),
            "chatId" => forged.chat_id = SageChatId::parse("chat-forged").unwrap(),
            "sessionId" => forged.session_id = SageAgentSessionId::parse("session-forged").unwrap(),
            "turnId" => forged.turn_id = SageTurnId::parse("turn-forged").unwrap(),
            "runtimeEpoch" => forged.runtime_epoch = RuntimeEpoch::ZERO,
            "requestId" => {
                forged.request_id = SageServerRequestId::parse("request-forged").unwrap()
            }
            _ => unreachable!("fixed forged field table"),
        }
        assert!(matches!(
            apply_request_state(&mut connection, forged),
            Err(StoreError::Conflict {
                resource: "request"
            })
        ));
        assert_eq!(request_snapshot(&connection), before, "{field}");
    }

    let duplicate = apply_request_state(&mut connection, exact).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(request_snapshot(&connection), before);
}

fn request_snapshot(connection: &Connection) -> (String, i64, i64, i64) {
    let (state, revision) = connection
        .query_row(
            "SELECT state,revision FROM server_requests WHERE id='request-one'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let projection_revision = connection
        .query_row(
            "SELECT projection_revision FROM agent_sessions WHERE id='session-one'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let fact_count = connection
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    (state, revision, projection_revision, fact_count)
}
