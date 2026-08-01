use rusqlite::params;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    domain::ChatMessage,
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
};

pub const MAX_BOOTSTRAP_MESSAGES: i64 = 500;
pub const MAX_BOOTSTRAP_MESSAGE_BYTES: i64 = 4 * 1024 * 1024;
pub const MAX_SESSION_EVENTS: i64 = 5_000;
pub const MAX_SESSION_EVENT_BYTES: i64 = 64 * 1024 * 1024;
pub const MAX_GLOBAL_EVENTS: i64 = 50_000;
pub const MAX_GLOBAL_EVENT_BYTES: i64 = 256 * 1024 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct MessagePayload {
    role: String,
    text: String,
}

#[derive(Clone, Copy)]
struct EventUsage {
    count: i64,
    bytes: i64,
}

impl Database {
    pub fn append_simulator_turn(
        &self,
        session_id: &str,
        user_text: &str,
        assistant_text: &str,
    ) -> AppResult<Vec<ChatMessage>> {
        self.append_simulator_turn_inner(session_id, user_text, assistant_text, || {})
    }

    #[cfg(test)]
    pub(crate) fn append_simulator_turn_with_hook(
        &self,
        session_id: &str,
        user_text: &str,
        assistant_text: &str,
        after_first_insert: impl FnOnce(),
    ) -> AppResult<Vec<ChatMessage>> {
        self.append_simulator_turn_inner(session_id, user_text, assistant_text, after_first_insert)
    }

    fn append_simulator_turn_inner(
        &self,
        session_id: &str,
        user_text: &str,
        assistant_text: &str,
        after_first_insert: impl FnOnce(),
    ) -> AppResult<Vec<ChatMessage>> {
        let payloads = [
            payload("user", user_text)?,
            payload("assistant", assistant_text)?,
        ];
        let added_bytes = payloads
            .iter()
            .try_fold(0_i64, |total, value| total.checked_add(value.len() as i64))
            .ok_or_else(|| AppError::ResourceLimit("session event bytes overflowed".into()))?;
        let mut connection = self.connection();
        let transaction = connection.transaction()?;
        let project_matches_active: bool = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1
               FROM agent_sessions s
               JOIN app_state a ON a.active_project_id = s.project_id
               WHERE s.id = ?1
             )",
            [session_id],
            |row| row.get(0),
        )?;
        if !project_matches_active {
            return Err(AppError::InvalidRequest(
                "session does not belong to the active project".into(),
            ));
        }
        let session_usage = session_usage(&transaction, session_id)?;
        let global_usage = global_usage(&transaction)?;
        ensure_turn_capacity(session_usage, global_usage, 2, added_bytes)?;

        let first_sequence: i64 = transaction.query_row(
            "SELECT COALESCE(MAX(sequence), 0) + 1
             FROM session_events WHERE session_id = ?1",
            [session_id],
            |row| row.get(0),
        )?;
        let created_at = now_millis();
        let mut after_first_insert = Some(after_first_insert);
        for (offset, payload) in payloads.iter().enumerate() {
            transaction.execute(
                "INSERT INTO session_events
                 (id, session_id, sequence, kind, payload_json, created_at)
                 VALUES (?1, ?2, ?3, 'message', ?4, ?5)",
                params![
                    Uuid::new_v4().to_string(),
                    session_id,
                    first_sequence + offset as i64,
                    payload,
                    created_at
                ],
            )?;
            if offset == 0 {
                after_first_insert.take().expect("turn hook is present")();
            }
        }
        let last_sequence = first_sequence + 1;
        transaction.execute(
            "UPDATE agent_sessions
             SET last_committed_sequence = ?2, updated_at = ?3
             WHERE id = ?1",
            params![session_id, last_sequence, created_at],
        )?;
        transaction.commit()?;
        drop(connection);
        self.messages(session_id)
    }

    pub fn messages(&self, session_id: &str) -> AppResult<Vec<ChatMessage>> {
        let connection = self.connection();
        let (count, bytes): (i64, i64) = connection.query_row(
            "SELECT COUNT(*), COALESCE(SUM(
               length(CAST(id AS BLOB)) + length(CAST(payload_json AS BLOB))
             ), 0)
             FROM (
               SELECT id, payload_json FROM session_events
               WHERE session_id = ?1 AND kind = 'message'
               ORDER BY sequence DESC LIMIT ?2
             )",
            params![session_id, MAX_BOOTSTRAP_MESSAGES],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        if count > MAX_BOOTSTRAP_MESSAGES || bytes > MAX_BOOTSTRAP_MESSAGE_BYTES {
            return Err(AppError::ResourceLimit(format!(
                "session bootstrap exceeds its {MAX_BOOTSTRAP_MESSAGES}-message / {} MiB limit",
                MAX_BOOTSTRAP_MESSAGE_BYTES / 1024 / 1024
            )));
        }
        let mut statement = connection.prepare(
            "SELECT id, sequence, payload_json, created_at
             FROM (
               SELECT id, sequence, payload_json, created_at
               FROM session_events
               WHERE session_id = ?1 AND kind = 'message'
               ORDER BY sequence DESC
               LIMIT ?2
             )
             ORDER BY sequence",
        )?;
        let rows = statement.query_map(params![session_id, MAX_BOOTSTRAP_MESSAGES], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
            ))
        })?;
        rows.map(|row| {
            let (id, sequence, payload, created_at) = row?;
            let payload: MessagePayload = serde_json::from_str(&payload)
                .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
            Ok(ChatMessage {
                id,
                session_id: session_id.into(),
                sequence,
                role: payload.role,
                text: payload.text,
                created_at,
            })
        })
        .collect()
    }
}

fn payload(role: &str, text: &str) -> AppResult<String> {
    serde_json::to_string(&MessagePayload {
        role: role.into(),
        text: text.into(),
    })
    .map_err(|error| AppError::InvalidRequest(error.to_string()))
}

fn session_usage(
    transaction: &rusqlite::Transaction<'_>,
    session_id: &str,
) -> AppResult<EventUsage> {
    let scan_limit = MAX_SESSION_EVENTS + 1;
    let count = transaction.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM session_events WHERE session_id = ?1 LIMIT ?2
         )",
        params![session_id, scan_limit],
        |row| row.get(0),
    )?;
    if count > MAX_SESSION_EVENTS {
        return Ok(EventUsage { count, bytes: 0 });
    }
    let bytes = transaction.query_row(
        "SELECT COALESCE(SUM(length(CAST(payload_json AS BLOB))), 0) FROM (
           SELECT payload_json FROM session_events WHERE session_id = ?1 LIMIT ?2
         )",
        params![session_id, scan_limit],
        |row| row.get(0),
    )?;
    Ok(EventUsage { count, bytes })
}

fn global_usage(transaction: &rusqlite::Transaction<'_>) -> AppResult<EventUsage> {
    let scan_limit = MAX_GLOBAL_EVENTS + 1;
    let count = transaction.query_row(
        "SELECT COUNT(*) FROM (SELECT 1 FROM session_events LIMIT ?1)",
        [scan_limit],
        |row| row.get(0),
    )?;
    if count > MAX_GLOBAL_EVENTS {
        return Ok(EventUsage { count, bytes: 0 });
    }
    let bytes = transaction.query_row(
        "SELECT COALESCE(SUM(length(CAST(payload_json AS BLOB))), 0) FROM (
           SELECT payload_json FROM session_events LIMIT ?1
         )",
        [scan_limit],
        |row| row.get(0),
    )?;
    Ok(EventUsage { count, bytes })
}

fn ensure_turn_capacity(
    session: EventUsage,
    global: EventUsage,
    added_count: i64,
    added_bytes: i64,
) -> AppResult<()> {
    let session_over = session.count > MAX_SESSION_EVENTS
        || session.bytes > MAX_SESSION_EVENT_BYTES
        || session.count.saturating_add(added_count) > MAX_SESSION_EVENTS
        || session.bytes.saturating_add(added_bytes) > MAX_SESSION_EVENT_BYTES;
    if session_over {
        return Err(AppError::ResourceLimit(format!(
            "session history capacity reached ({MAX_SESSION_EVENTS} events / {} MiB); existing history was preserved",
            MAX_SESSION_EVENT_BYTES / 1024 / 1024
        )));
    }
    let global_over = global.count > MAX_GLOBAL_EVENTS
        || global.bytes > MAX_GLOBAL_EVENT_BYTES
        || global.count.saturating_add(added_count) > MAX_GLOBAL_EVENTS
        || global.bytes.saturating_add(added_bytes) > MAX_GLOBAL_EVENT_BYTES;
    if global_over {
        return Err(AppError::ResourceLimit(format!(
            "global history capacity reached ({MAX_GLOBAL_EVENTS} events / {} MiB); existing history was preserved",
            MAX_GLOBAL_EVENT_BYTES / 1024 / 1024
        )));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, sync::mpsc, thread, time::Duration};

    use uuid::Uuid;

    use super::*;
    use crate::domain::{ProjectLayoutRequest, ProjectSwitchRequest};

    fn session(database: &Database) -> String {
        let root = std::env::temp_dir().join(format!("sage-events-root-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let project = database.create_project_from_root(&root).unwrap();
        fs::remove_dir_all(root).unwrap();
        database
            .connection()
            .query_row(
                "SELECT id FROM agent_sessions WHERE project_id = ?1",
                [project.id],
                |row| row.get(0),
            )
            .unwrap()
    }

    #[test]
    fn simulator_turn_commits_both_messages() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        let messages = database
            .append_simulator_turn(&session_id, "hello", "ack")
            .unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[1].role, "user");
        assert_eq!(messages[2].role, "assistant");
    }

    #[test]
    fn count_limit_accepts_exact_capacity_then_rejects_without_partial_turn() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        database
            .connection()
            .execute(
                "WITH RECURSIVE n(value) AS (
                   SELECT 2 UNION ALL SELECT value + 1 FROM n WHERE value < ?2
                 )
                 INSERT INTO session_events
                   (id, session_id, sequence, kind, payload_json, created_at)
                 SELECT printf('fixture-%d', value), ?1, value, 'message',
                        '{\"role\":\"user\",\"text\":\"x\"}', 1 FROM n",
                params![session_id, MAX_SESSION_EVENTS - 2],
            )
            .unwrap();
        database
            .append_simulator_turn(&session_id, "at limit", "ack")
            .unwrap();
        let before: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, MAX_SESSION_EVENTS);
        assert!(
            database
                .append_simulator_turn(&session_id, "overflow", "ack")
                .is_err()
        );
        let after: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn global_count_limit_rejects_a_whole_turn_at_exact_capacity() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        let connection = database.connection();
        connection
            .execute(
                "WITH RECURSIVE n(value) AS (
                   SELECT 0 UNION ALL SELECT value + 1 FROM n WHERE value < 10
                 )
                 INSERT INTO agent_sessions
                   (id, project_id, chat_tab_id, harness_id, harness_version,
                    model_id, external_session_id, status,
                    last_committed_sequence, created_at, updated_at)
                 SELECT printf('global-session-%d', n.value), source.project_id,
                        source.chat_tab_id, source.harness_id,
                        source.harness_version, source.model_id, NULL, 'idle',
                        0, 1, 1
                 FROM n JOIN agent_sessions source ON source.id = ?1",
                [&session_id],
            )
            .unwrap();
        connection
            .execute(
                "WITH RECURSIVE n(value) AS (
                   SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < ?1
                 )
                 INSERT INTO session_events
                   (id, session_id, sequence, kind, payload_json, created_at)
                 SELECT printf('global-fixture-%d', value),
                        printf('global-session-%d', (value - 1) / 4999),
                        ((value - 1) % 4999) + 1, 'message',
                        '{\"role\":\"user\",\"text\":\"x\"}', 1 FROM n",
                [MAX_GLOBAL_EVENTS - 1],
            )
            .unwrap();
        let before: i64 = connection
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, MAX_GLOBAL_EVENTS);
        drop(connection);

        assert!(
            database
                .append_simulator_turn(&session_id, "overflow", "ack")
                .is_err()
        );
        let after: i64 = database
            .connection()
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, before);
    }

    #[test]
    fn existing_over_limit_usage_is_preserved_and_rejected() {
        let usage = EventUsage {
            count: MAX_SESSION_EVENTS + 1,
            bytes: 1,
        };
        let result = ensure_turn_capacity(usage, usage, 2, 10);
        assert!(matches!(result, Err(AppError::ResourceLimit(_))));
    }

    #[test]
    fn existing_over_limit_rows_survive_reopen_and_reject_new_turns() {
        let base = std::env::temp_dir().join(format!("sage-over-limit-{}", Uuid::new_v4()));
        fs::create_dir_all(&base).unwrap();
        let database_path = base.join("sage.db");
        let database = Database::open(&database_path).unwrap();
        let session_id = session(&database);
        database
            .connection()
            .execute(
                "WITH RECURSIVE n(value) AS (
                   SELECT 2 UNION ALL SELECT value + 1 FROM n WHERE value <= ?2
                 )
                 INSERT INTO session_events
                   (id, session_id, sequence, kind, payload_json, created_at)
                 SELECT printf('legacy-%d', value), ?1, value, 'message',
                        '{\"role\":\"user\",\"text\":\"legacy\"}', 1 FROM n",
                params![session_id, MAX_SESSION_EVENTS],
            )
            .unwrap();
        drop(database);

        let reopened = Database::open(&database_path).unwrap();
        let before: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(before, MAX_SESSION_EVENTS + 1);
        assert!(
            reopened
                .append_simulator_turn(&session_id, "new", "assistant")
                .is_err()
        );
        let after: i64 = reopened
            .connection()
            .query_row("SELECT COUNT(*) FROM session_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(after, before);
        drop(reopened);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn aggregate_byte_limits_reject_before_append() {
        let session = EventUsage {
            count: 1,
            bytes: MAX_SESSION_EVENT_BYTES - 1,
        };
        let global = EventUsage { count: 1, bytes: 1 };
        assert!(ensure_turn_capacity(session, global, 2, 2).is_err());
        let session = EventUsage { count: 1, bytes: 1 };
        let global = EventUsage {
            count: 1,
            bytes: MAX_GLOBAL_EVENT_BYTES - 1,
        };
        assert!(ensure_turn_capacity(session, global, 2, 2).is_err());
    }

    #[test]
    fn transcript_bootstrap_rejects_actual_stored_bytes_before_json_collection() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        database
            .connection()
            .execute(
                "UPDATE session_events SET payload_json = ?2 WHERE session_id = ?1",
                params![
                    session_id,
                    "x".repeat(MAX_BOOTSTRAP_MESSAGE_BYTES as usize + 1)
                ],
            )
            .unwrap();
        assert!(matches!(
            database.messages(&session_id),
            Err(AppError::ResourceLimit(_))
        ));
    }

    #[test]
    fn transcript_bootstrap_collects_only_the_latest_bounded_window() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        database
            .connection()
            .execute(
                "WITH RECURSIVE n(value) AS (
                   SELECT 2 UNION ALL SELECT value + 1 FROM n WHERE value <= ?2
                 )
                 INSERT INTO session_events
                   (id, session_id, sequence, kind, payload_json, created_at)
                 SELECT printf('bootstrap-%d', value), ?1, value, 'message',
                        '{\"role\":\"user\",\"text\":\"x\"}', 1 FROM n",
                params![session_id, MAX_BOOTSTRAP_MESSAGES],
            )
            .unwrap();
        let messages = database.messages(&session_id).unwrap();
        assert_eq!(messages.len(), MAX_BOOTSTRAP_MESSAGES as usize);
        assert_eq!(messages.first().unwrap().sequence, 2);
        assert_eq!(
            messages.last().unwrap().sequence,
            MAX_BOOTSTRAP_MESSAGES + 1
        );
    }

    #[test]
    fn switch_cannot_observe_or_leave_a_partial_turn_and_restart_is_stable() {
        let base = std::env::temp_dir().join(format!("sage-turn-race-{}", Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let database_path = base.join("sage.db");
        let database = Database::open(&database_path).unwrap();
        let project_a = database.create_project_from_root(&root_a).unwrap();
        let session_a: String = database
            .connection()
            .query_row(
                "SELECT id FROM agent_sessions WHERE project_id = ?1",
                [&project_a.id],
                |row| row.get(0),
            )
            .unwrap();
        let project_b = database.create_project_from_root(&root_b).unwrap();
        let current_b = database.bootstrap_snapshot().unwrap();
        database
            .switch_project(&ProjectSwitchRequest {
                target_project_id: project_a.id.clone(),
                current_layout: layout(&current_b),
            })
            .unwrap();
        let current_a = database.bootstrap_snapshot().unwrap();
        let switch = ProjectSwitchRequest {
            target_project_id: project_b.id.clone(),
            current_layout: layout(&current_a),
        };

        let (inserted_tx, inserted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let turn_database = database.clone();
        let turn_session = session_a.clone();
        let turn = thread::spawn(move || {
            turn_database.append_simulator_turn_with_hook(
                &turn_session,
                "user",
                "assistant",
                || {
                    inserted_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                },
            )
        });
        inserted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (switch_started_tx, switch_started_rx) = mpsc::channel();
        let switch_database = database.clone();
        let switching = thread::spawn(move || {
            switch_started_tx.send(()).unwrap();
            switch_database.switch_project(&switch)
        });
        switch_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        release_tx.send(()).unwrap();
        turn.join().unwrap().unwrap();
        switching.join().unwrap().unwrap();
        drop(database);

        let reopened = Database::open(&database_path).unwrap();
        assert_eq!(reopened.messages(&session_a).unwrap().len(), 3);
        assert_eq!(
            reopened.active_project_id().unwrap().as_deref(),
            Some(project_b.id.as_str())
        );
        drop(reopened);
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn failed_switch_waits_for_a_latched_turn_and_leaves_both_messages() {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session_id = session(&database);
        let snapshot = database.bootstrap_snapshot().unwrap();
        let request = ProjectSwitchRequest {
            target_project_id: "missing".into(),
            current_layout: layout(&snapshot),
        };
        let (inserted_tx, inserted_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let turn_database = database.clone();
        let turn_session = session_id.clone();
        let turn = thread::spawn(move || {
            turn_database.append_simulator_turn_with_hook(
                &turn_session,
                "user",
                "assistant",
                || {
                    inserted_tx.send(()).unwrap();
                    release_rx.recv_timeout(Duration::from_secs(2)).unwrap();
                },
            )
        });
        inserted_rx.recv_timeout(Duration::from_secs(2)).unwrap();
        let (switch_started_tx, switch_started_rx) = mpsc::channel();
        let switch_database = database.clone();
        let switching = thread::spawn(move || {
            switch_started_tx.send(()).unwrap();
            switch_database.switch_project(&request)
        });
        switch_started_rx
            .recv_timeout(Duration::from_secs(2))
            .unwrap();
        release_tx.send(()).unwrap();
        turn.join().unwrap().unwrap();
        assert!(switching.join().unwrap().is_err());
        assert_eq!(database.messages(&session_id).unwrap().len(), 3);
    }

    #[test]
    fn switch_first_rejects_the_old_session_before_either_message_is_inserted() {
        let base = std::env::temp_dir().join(format!("sage-switch-first-{}", Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        fs::create_dir_all(&root_a).unwrap();
        fs::create_dir_all(&root_b).unwrap();
        let database_path = base.join("sage.db");
        let database = Database::open(&database_path).unwrap();
        let project_a = database.create_project_from_root(&root_a).unwrap();
        let session_a: String = database
            .connection()
            .query_row(
                "SELECT id FROM agent_sessions WHERE project_id = ?1",
                [&project_a.id],
                |row| row.get(0),
            )
            .unwrap();
        database.create_project_from_root(&root_b).unwrap();

        let error =
            database.append_simulator_turn_with_hook(&session_a, "user", "assistant", || {
                panic!("inactive-session validation must precede the first insert")
            });
        assert!(error.is_err());
        assert_eq!(database.messages(&session_a).unwrap().len(), 1);
        drop(database);

        let reopened = Database::open(&database_path).unwrap();
        assert_eq!(reopened.messages(&session_a).unwrap().len(), 1);
        drop(reopened);
        fs::remove_dir_all(base).unwrap();
    }

    fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
        ProjectLayoutRequest {
            project_id: snapshot.project.as_ref().unwrap().id.clone(),
            tabs: snapshot.tabs.clone(),
            active_tab_id: snapshot.active_tab_id.clone(),
            selected_root_id: snapshot.selected_root_id.clone(),
            selected_resource: snapshot.selected_resource.clone(),
        }
    }
}
