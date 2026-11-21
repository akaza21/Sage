//! Native evidence keeps its terminal instant across command cleanup and replay.
use super::super::{StoreError, facts};
use rusqlite::{Connection, OptionalExtension, params};

const REFUSAL_SQL: &str = "SELECT created_at FROM session_facts WHERE project_id=?1 AND chat_id=?2
         AND kind='commandSignInRefusal' AND json_extract(payload_json,'$.clientIntentId')=?3 LIMIT 1";
const TERMINAL_SQL: &str = "SELECT terminal_at FROM turns
         WHERE project_id=?1 AND chat_id=?2 AND client_intent_id=?3 AND session_id=?4 AND terminal_at IS NOT NULL
         UNION ALL
         SELECT terminal_at FROM agent_sessions
         WHERE project_id=?1 AND chat_id=?2 AND id=?4 AND terminal_at IS NOT NULL
           AND EXISTS(SELECT 1 FROM session_facts WHERE project_id=?1 AND chat_id=?2 AND session_id=?4
             AND kind IN ('sessionStartIntent','sessionActivateIntent')
             AND json_extract(payload_json,'$.clientIntentId')=?3)
         LIMIT 1";
const SESSION_SQL: &str = "SELECT session_id FROM session_facts WHERE project_id=?1 AND chat_id=?2
             AND kind IN ('sessionStartIntent','sessionActivateIntent')
             AND json_extract(payload_json,'$.clientIntentId')=?3 LIMIT 1";

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
    let original = transaction
        .query_row(
            REFUSAL_SQL,
            params![
                command.project_id,
                command.chat_id,
                command.client_intent_id
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?;
    if let Some(original) = original {
        return Ok(original);
    }
    // Start requests have no session ID yet. Resolve the sparse command intent
    // once, then use primary/unique keys for the terminal resource itself.
    let session_id = match command.session_id {
        Some(id) => id.to_owned(),
        None => transaction.query_row(
            SESSION_SQL,
            params![
                command.project_id,
                command.chat_id,
                command.client_intent_id
            ],
            |row| row.get::<_, String>(0),
        )?,
    };
    let terminal_at = transaction
        .query_row(
            TERMINAL_SQL,
            params![
                command.project_id,
                command.chat_id,
                command.client_intent_id,
                session_id,
            ],
            |row| row.get::<_, i64>(0),
        )
        .optional()?
        .unwrap_or(now_millis);

    // Configuration commands need no terminal Turn or Session. Persist their
    // own terminal fact through the same capacity-accounted native ledger.
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
    fn refusal_evidence_seeks_its_command_amid_unrelated_history() {
        let mut connection = crate::acp::store::tests::setup();
        connection
            .execute_batch(include_str!(
                "../../../../migrations/033_sign_in_evidence_indexes.sql"
            ))
            .unwrap();
        connection.execute_batch(
            "WITH RECURSIVE n(x) AS (VALUES(1) UNION ALL SELECT x+1 FROM n WHERE x<10000)
             INSERT INTO session_facts(project_id,chat_id,session_id,sequence,kind,payload_json)
             SELECT 'project-one','chat-one','session-one',x,'adapterBatch','{}' FROM n;
             INSERT INTO session_facts(project_id,chat_id,session_id,sequence,kind,payload_json)
             VALUES('project-one','chat-one','session-one',10001,'sessionStartIntent','{\"clientIntentId\":\"start\"}');
             UPDATE agent_sessions SET terminal_at=150;"
        ).unwrap();
        let mut start = command("start");
        start.session_id = None;
        assert_eq!(
            command_terminal_at(&mut connection, start, 400).unwrap(),
            150
        );
        assert_eq!(
            command_terminal_at(&mut connection, command("start"), 600).unwrap(),
            150
        );
        assert_eq!(
            command_terminal_at(&mut connection, command("config"), 700).unwrap(),
            700
        );
        for (sql, index) in [
            (REFUSAL_SQL, "idx_session_facts_sign_in_refusal"),
            (SESSION_SQL, "idx_session_facts_session_intent"),
        ] {
            let mut statement = connection
                .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
                .unwrap();
            let plan = statement
                .query_map(params!["project-one", "chat-one", "start"], |row| {
                    row.get::<_, String>(3)
                })
                .unwrap()
                .collect::<Result<Vec<_>, _>>()
                .unwrap();
            assert!(
                plan.iter().any(|step| step.contains("SEARCH")
                    && step.contains(index)
                    && step.contains("<expr>=")),
                "{plan:?}"
            );
            assert!(
                !plan.iter().any(|step| step.starts_with("SCAN")),
                "{plan:?}"
            );
        }
        let mut statement = connection
            .prepare(&format!("EXPLAIN QUERY PLAN {TERMINAL_SQL}"))
            .unwrap();
        let plan = statement
            .query_map(
                params!["project-one", "chat-one", "start", "session-one"],
                |row| row.get::<_, String>(3),
            )
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(
            !plan.iter().any(|step| step.starts_with("SCAN")),
            "{plan:?}"
        );
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
