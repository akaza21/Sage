use rusqlite::{Transaction, params};
use sage_core::error::{AppError, AppResult};

use sage_core::limits::LIMITS;
use sage_domain::chat::MoveChatRequest;

pub(super) fn move_in(
    transaction: &Transaction<'_>,
    request: &MoveChatRequest,
    turn: &str,
) -> AppResult<()> {
    let (session, bytes): (String, i64) = transaction.query_row(
        "SELECT t.session_id,u.payload_bytes FROM turns t
         JOIN acp_session_usage u ON u.session_id=t.session_id
         WHERE t.id=?1 AND t.project_id=?2 AND t.chat_id=?3",
        params![turn, request.project_id, request.chat_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let changed = transaction.execute(
        "UPDATE acp_project_usage SET session_count=session_count+1,turn_count=turn_count+1,
         item_count=item_count+1,fact_count=fact_count+1,payload_bytes=payload_bytes+?2,
         revision=revision+1 WHERE project_id=?1 AND session_count<?3 AND turn_count<?4",
        params![
            request.target_project_id,
            bytes,
            LIMITS.durable.sessions_per_project as i64,
            LIMITS.durable.turns_per_project as i64
        ],
    )?;
    if changed != 1 {
        return Err(AppError::ResourceLimit(
            "Target Project cannot admit legacy welcome history".into(),
        ));
    }
    transaction.execute(
        "UPDATE acp_project_usage SET session_count=session_count-1,turn_count=turn_count-1,
         item_count=item_count-1,fact_count=fact_count-1,payload_bytes=payload_bytes-?2,
         revision=revision+1 WHERE project_id=?1",
        params![request.project_id, bytes],
    )?;
    let mut guards = Vec::new();
    for name in [
        "deny_agent_session_identity_mutation",
        "deny_runtime_epoch_identity_mutation",
        "deny_turn_identity_mutation",
        "deny_terminal_item_mutation",
        "deny_fact_update",
    ] {
        let sql: String = transaction.query_row(
            "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
            [name],
            |row| row.get(0),
        )?;
        guards.push(sql);
        transaction.execute_batch(&format!("DROP TRIGGER {name}"))?;
    }
    transaction.execute(
        "UPDATE agent_sessions SET project_id=?2 WHERE project_id=?1 AND chat_id=?3 AND id=?4",
        params![
            request.project_id,
            request.target_project_id,
            request.chat_id,
            session
        ],
    )?;
    for table in [
        "session_runtime_epochs",
        "turns",
        "items",
        "session_facts",
        "acp_session_usage",
    ] {
        let changed = transaction.execute(
            &format!("UPDATE {table} SET project_id=?2 WHERE project_id=?1 AND chat_id=?3 AND session_id=?4"),
            params![request.project_id, request.target_project_id, request.chat_id, session],
        )?;
        if changed != 1 {
            return Err(AppError::InvalidRequest(
                "Legacy welcome ownership is inconsistent".into(),
            ));
        }
    }
    for sql in guards {
        transaction.execute_batch(&sql)?;
    }
    Ok(())
}
