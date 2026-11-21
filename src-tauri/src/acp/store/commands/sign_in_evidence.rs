//! Native evidence keeps its terminal instant across command cleanup and replay.
use super::super::{StoreError, facts};
use rusqlite::{Connection, OptionalExtension, params};

pub(crate) struct RefusalCommand<'a> {
    pub project_id: &'a str,
    pub chat_id: &'a str,
    pub client_intent_id: &'a str,
    pub session_id: Option<&'a str>,
}

pub(crate) fn command_terminal_at(
    connection: &mut Connection,
    command: RefusalCommand<'_>,
    now_millis: i64,
) -> Result<i64, StoreError> {
    let transaction = connection.transaction()?;
    let original = transaction.query_row(
        "SELECT created_at FROM session_facts WHERE project_id=?1 AND chat_id=?2
         AND kind='commandSignInRefusal' AND json_extract(payload_json,'$.clientIntentId')=?3 LIMIT 1",
        params![command.project_id, command.chat_id, command.client_intent_id], |row| row.get::<_, i64>(0),
    ).optional()?;
    if let Some(original) = original {
        return Ok(original);
    }
    let terminal_at = transaction
        .query_row(
            "SELECT terminal_at FROM turns
         WHERE project_id=?1 AND chat_id=?2 AND client_intent_id=?3 AND terminal_at IS NOT NULL
         UNION ALL
         SELECT s.terminal_at FROM session_facts f JOIN agent_sessions s ON s.id=f.session_id
         WHERE f.project_id=?1 AND f.chat_id=?2
           AND f.kind IN ('sessionStartIntent','sessionActivateIntent')
           AND json_extract(f.payload_json,'$.clientIntentId')=?3 AND s.terminal_at IS NOT NULL
         LIMIT 1",
            params![
                command.project_id,
                command.chat_id,
                command.client_intent_id
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(now_millis);

    // Configuration commands need no terminal Turn or Session. Persist their
    // own terminal fact through the same capacity-accounted native ledger.
    let session_id = match command.session_id {
        Some(id) => id.to_owned(),
        None => transaction.query_row(
            "SELECT session_id FROM session_facts WHERE project_id=?1 AND chat_id=?2
             AND kind IN ('sessionStartIntent','sessionActivateIntent')
             AND json_extract(payload_json,'$.clientIntentId')=?3 LIMIT 1",
            params![
                command.project_id,
                command.chat_id,
                command.client_intent_id
            ],
            |row| row.get::<_, String>(0),
        )?,
    };
    let revision = transaction.query_row(
        "SELECT projection_revision FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 AND id=?3",
        params![command.project_id, command.chat_id, session_id], |row| row.get::<_, i64>(0),
    )?;
    let payload = serde_json::json!({ "clientIntentId": command.client_intent_id }).to_string();
    facts::append_fact_payload(
        &transaction,
        command.project_id,
        command.chat_id,
        &session_id,
        None,
        "commandSignInRefusal",
        &payload,
        facts::projection_from_i64(revision)?,
        terminal_at,
    )?;
    transaction.commit()?;
    Ok(terminal_at)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn command(intent: &str) -> RefusalCommand<'_> {
        RefusalCommand {
            project_id: "project-one",
            chat_id: "chat-one",
            client_intent_id: intent,
            session_id: Some("session-one"),
        }
    }
    #[test]
    fn command_refusal_is_persisted_once_and_replays_its_original_terminal_time() {
        let mut connection = crate::acp::store::tests::setup();
        assert_eq!(
            command_terminal_at(&mut connection, command("config"), 200).unwrap(),
            200
        );
        assert_eq!(
            command_terminal_at(&mut connection, command("config"), 400).unwrap(),
            200
        );
        let facts: i64 = connection
            .query_row(
                "SELECT fact_count FROM acp_session_usage WHERE session_id='session-one'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(facts, 1);
    }
    #[test]
    fn refused_turn_and_session_use_durable_terminal_time_before_delayed_cleanup() {
        let mut connection = crate::acp::store::tests::setup();
        connection.execute_batch("INSERT INTO turns(id,project_id,chat_id,session_id,client_intent_id,terminal_at) VALUES('turn','project-one','chat-one','session-one','send',200);
          UPDATE agent_sessions SET terminal_at=150;
          INSERT INTO session_facts(project_id,chat_id,session_id,sequence,kind,payload_json) VALUES('project-one','chat-one','session-one',1,'sessionStartIntent','{\"clientIntentId\":\"start\"}');").unwrap();
        assert_eq!(
            command_terminal_at(&mut connection, command("send"), 400).unwrap(),
            200
        );
        assert_eq!(
            command_terminal_at(&mut connection, command("start"), 400).unwrap(),
            150
        );
        connection
            .execute("UPDATE agent_sessions SET terminal_at=500", [])
            .unwrap();
        assert_eq!(
            command_terminal_at(&mut connection, command("start"), 600).unwrap(),
            150
        );
    }
    #[test]
    fn missing_owner_or_exhausted_ledger_never_fabricates_evidence() {
        let mut connection = crate::acp::store::tests::setup();
        let mut foreign = command("config");
        foreign.session_id = Some("foreign");
        assert!(command_terminal_at(&mut connection, foreign, 200).is_err());
        connection
            .execute(
                "UPDATE acp_session_usage SET fact_count=?1",
                [sage_core::limits::LIMITS.durable.facts_per_session as i64],
            )
            .unwrap();
        assert!(matches!(
            command_terminal_at(&mut connection, command("config"), 200),
            Err(StoreError::Capacity { .. })
        ));
        let count: i64 = connection
            .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
