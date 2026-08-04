use rusqlite::{OptionalExtension, Transaction, params};
use uuid::Uuid;

use crate::{
    acp::limits::LIMITS,
    error::{AppError, AppResult},
    harnesses::SIMULATOR_ID,
};

use super::{SIMULATOR_CAPABILITIES, digest, require_one};

pub(super) struct OwnedOwner {
    pub(super) project: String,
    pub(super) chat: String,
    pub(super) session: String,
}

pub(super) fn resolve_or_create(
    transaction: &Transaction<'_>,
    chat_id: &str,
    model_id: &str,
    now: i64,
) -> AppResult<OwnedOwner> {
    let project: String = transaction
        .query_row(
            "SELECT c.project_id FROM chats c
             JOIN app_state a ON a.active_project_id=c.project_id
             WHERE c.id=?1",
            [chat_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| AppError::InvalidRequest("Chat is not in the active project".into()))?;
    let binding_digest = digest(format!("sage-simulator-binding-v1\0{SIMULATOR_ID}").as_bytes());
    if let Some((session, current_model)) =
        compatible_head(transaction, &project, chat_id, &binding_digest)?
    {
        if current_model == model_id {
            return Ok(OwnedOwner {
                project,
                chat: chat_id.into(),
                session,
            });
        }
        close_head(transaction, &session, now)?;
    }
    create_owner(transaction, project, chat_id, binding_digest, model_id, now)
}

fn compatible_head(
    transaction: &Transaction<'_>,
    project: &str,
    chat_id: &str,
    binding_digest: &str,
) -> AppResult<Option<(String, String)>> {
    let head: Option<(String, String, String, String, i64, Option<String>)> = transaction
        .query_row(
            "SELECT s.id,s.binding_id,s.binding_digest,s.state,s.legacy,s.model_id
             FROM chat_session_heads h
             JOIN agent_sessions s
               ON s.project_id=h.project_id AND s.chat_id=h.chat_id AND s.id=h.session_id
             WHERE h.project_id=?1 AND h.chat_id=?2",
            params![project, chat_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((session, binding, stored_digest, state, legacy, model)) = head else {
        return Ok(None);
    };
    if binding != SIMULATOR_ID || stored_digest != binding_digest || state != "ready" || legacy != 0
    {
        return Err(AppError::InvalidRequest(
            "active Chat head is not a compatible simulator session".into(),
        ));
    }
    let model = model.ok_or_else(|| {
        AppError::InvalidRequest("active simulator head has no model identity".into())
    })?;
    Ok(Some((session, model)))
}

fn close_head(transaction: &Transaction<'_>, session: &str, now: i64) -> AppResult<()> {
    require_one(
        transaction.execute(
            "DELETE FROM chat_session_heads WHERE session_id=?1",
            [session],
        )?,
        "simulator Chat head disappeared during replacement",
    )?;
    require_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='closing',revision=revision+1,updated_at=?2
             WHERE id=?1 AND state='ready' AND residency='cold' AND legacy=0",
            params![session, now],
        )?,
        "active simulator head is not replaceable",
    )?;
    require_one(
        transaction.execute(
            "UPDATE agent_sessions SET state='closed',revision=revision+1,
                    updated_at=?2,terminal_at=?2 WHERE id=?1 AND state='closing'",
            params![session, now],
        )?,
        "simulator head did not close during replacement",
    )
}

fn create_owner(
    transaction: &Transaction<'_>,
    project: String,
    chat_id: &str,
    binding_digest: String,
    model_id: &str,
    now: i64,
) -> AppResult<OwnedOwner> {
    let chat_sessions: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM agent_sessions WHERE project_id=?1 AND chat_id=?2 LIMIT ?3
         )",
        params![
            project,
            chat_id,
            LIMITS.durable.sessions_per_chat as i64 + 1
        ],
        |row| row.get(0),
    )?;
    let project_sessions: i64 = transaction.query_row(
        "SELECT session_count FROM acp_project_usage WHERE project_id=?1",
        [&project],
        |row| row.get(0),
    )?;
    let global_sessions: i64 = transaction.query_row(
        "SELECT session_count FROM acp_store_usage WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if chat_sessions >= LIMITS.durable.sessions_per_chat as i64
        || project_sessions >= LIMITS.durable.sessions_per_project as i64
        || global_sessions >= LIMITS.durable.sessions_global as i64
    {
        return Err(AppError::ResourceLimit(
            "simulator session capacity reached; existing history was preserved".into(),
        ));
    }
    let ordinal: i64 = transaction.query_row(
        "SELECT COALESCE(MAX(ordinal),-1)+1 FROM agent_sessions
         WHERE project_id=?1 AND chat_id=?2",
        params![project, chat_id],
        |row| row.get(0),
    )?;
    if ordinal < 0 || ordinal >= LIMITS.durable.sessions_per_chat as i64 {
        return Err(AppError::ResourceLimit(
            "simulator session ordinal capacity reached".into(),
        ));
    }
    let installed_revision = next_chat_projection(transaction, &project, chat_id)?;
    let session = Uuid::new_v4().to_string();
    transaction.execute(
        "INSERT INTO agent_sessions(
           id,project_id,chat_id,ordinal,binding_id,binding_digest,model_id,
           external_session_id,capabilities_json,state,residency,legacy,revision,
           projection_revision,created_at,updated_at,terminal_at
         ) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,'ready','cold',0,0,?10,?11,?11,NULL)",
        params![
            session,
            project,
            chat_id,
            ordinal,
            SIMULATOR_ID,
            binding_digest,
            model_id,
            format!("sage-simulator:{session}"),
            SIMULATOR_CAPABILITIES,
            installed_revision,
            now
        ],
    )?;
    transaction.execute(
        "INSERT INTO acp_session_usage(
           project_id,chat_id,session_id,turn_count,item_count,item_bytes,
           fact_count,fact_bytes,payload_bytes,revision
         ) VALUES (?1,?2,?3,0,0,0,0,0,0,0)",
        params![project, chat_id, session],
    )?;
    require_one(
        transaction.execute(
            "UPDATE acp_project_usage SET session_count=session_count+1,revision=revision+1
             WHERE project_id=?1 AND session_count=?2",
            params![project, project_sessions],
        )?,
        "simulator project session ledger changed",
    )?;
    require_one(
        transaction.execute(
            "UPDATE acp_store_usage SET session_count=session_count+1,revision=revision+1
             WHERE singleton=1 AND session_count=?1",
            [global_sessions],
        )?,
        "global simulator session ledger changed",
    )?;
    transaction.execute(
        "INSERT INTO chat_session_heads(
           project_id,chat_id,session_id,installed_revision,installed_at
         ) VALUES (?1,?2,?3,?4,?5)",
        params![project, chat_id, session, installed_revision, now],
    )?;
    Ok(OwnedOwner {
        project,
        chat: chat_id.into(),
        session,
    })
}

pub(super) fn next_chat_projection(
    transaction: &Transaction<'_>,
    project_id: &str,
    chat_id: &str,
) -> AppResult<i64> {
    let current: i64 = transaction.query_row(
        "SELECT projection_revision FROM chats WHERE project_id=?1 AND id=?2",
        params![project_id, chat_id],
        |row| row.get(0),
    )?;
    current
        .checked_add(1)
        .ok_or_else(|| AppError::ResourceLimit("Chat projection revision exhausted".into()))
}
