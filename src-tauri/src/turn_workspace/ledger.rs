use std::path::Path;

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use super::{TurnWorkspaceChanges, WorkspaceMark};
use crate::acp::store::StoreError;
use sage_core::limits::LIMITS;

const MAX_ACTIVE_BASELINES: i64 = 64;
const MAX_PAYLOAD_BYTES: usize = 128 * 1024;

#[cfg(test)]
#[path = "ledger_tests.rs"]
mod tests;

pub(crate) fn remember(
    connection: &mut Connection,
    session: &str,
    key: &str,
    root_id: &str,
    root: &Path,
    mark: Option<WorkspaceMark>,
) -> Result<bool, StoreError> {
    let transaction = connection.transaction()?;
    let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM acp_turn_workspace_changes WHERE session_id=?1 AND client_intent_id=?2)", params![session,key], |row| row.get(0))?;
    if exists {
        return Ok(false);
    }
    let session_exists: bool = transaction.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id=?1)",
        [session],
        |row| row.get(0),
    )?;
    if !session_exists {
        return Ok(false);
    }
    let active: i64 = transaction.query_row(
        "SELECT COUNT(*) FROM acp_turn_workspace_changes WHERE result_json IS NULL",
        [],
        |row| row.get(0),
    )?;
    if active >= MAX_ACTIVE_BASELINES {
        return Err(StoreError::Capacity {
            resource: "workspaceBaselines",
            limit: MAX_ACTIVE_BASELINES as u64,
        });
    }
    let root = root
        .to_str()
        .ok_or(StoreError::Corrupt("workspace root encoding"))?;
    let overlaps: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM acp_turn_workspace_changes WHERE root_path=?1 AND result_json IS NULL)", [root], |row| row.get(0))?;
    transaction.execute("UPDATE acp_turn_workspace_changes SET ambiguous=1 WHERE root_path=?1 AND result_json IS NULL", [root])?;
    let baseline = mark
        .and_then(|mark| serde_json::to_string(&mark).ok())
        .filter(|json| json.len() + root.len() + root_id.len() <= MAX_PAYLOAD_BYTES / 2);
    let bytes = MAX_PAYLOAD_BYTES as i64;
    charge(&transaction, session, bytes)?;
    transaction.execute("INSERT INTO acp_turn_workspace_changes(project_id,session_id,client_intent_id,root_id,root_path,baseline_json,ambiguous,payload_bytes) SELECT project_id,?1,?2,?3,?4,?5,?6,?7 FROM agent_sessions WHERE id=?1", params![session,key,root_id,root,baseline,overlaps,bytes])?;
    transaction
        .commit()
        .map_err(|_| StoreError::CommitUnknown)?;
    Ok(true)
}

pub(crate) fn publish_baseline(
    connection: &Connection,
    session: &str,
    key: &str,
    mark: Option<WorkspaceMark>,
) -> Result<(), StoreError> {
    let json = mark
        .and_then(|mark| serde_json::to_string(&mark).ok())
        .filter(|json| json.len() <= MAX_PAYLOAD_BYTES / 2);
    connection.execute("UPDATE acp_turn_workspace_changes SET baseline_json=CASE WHEN length(CAST(?3 AS BLOB))+length(CAST(root_path AS BLOB))+length(CAST(root_id AS BLOB))<=payload_bytes/2 THEN ?3 ELSE NULL END WHERE session_id=?1 AND client_intent_id=?2 AND result_json IS NULL AND terminal_json IS NULL", params![session,key,json])?;
    Ok(())
}

pub(crate) fn publish_baseline_failure(
    connection: &Connection,
    session: &str,
    key: &str,
) -> Result<(), StoreError> {
    connection.execute("UPDATE acp_turn_workspace_changes SET baseline_json=?3 WHERE session_id=?1 AND client_intent_id=?2 AND result_json IS NULL AND terminal_json IS NULL", params![session,key,super::FAILED_BASELINE_JSON])?;
    Ok(())
}

pub(crate) fn forget_unprepared(
    connection: &Connection,
    session: &str,
    key: &str,
) -> Result<(), StoreError> {
    connection.execute("DELETE FROM acp_turn_workspace_changes WHERE session_id=?1 AND client_intent_id=?2 AND NOT EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND client_intent_id=?2)", params![session,key])?;
    Ok(())
}

pub(crate) fn invalidate_active(connection: &Connection) -> Result<(), StoreError> {
    connection.execute(
        "UPDATE acp_turn_workspace_changes SET ambiguous=1 WHERE result_json IS NULL",
        [],
    )?;
    connection.execute("DELETE FROM acp_turn_workspace_changes WHERE result_json IS NULL AND NOT EXISTS(SELECT 1 FROM turns t WHERE t.session_id=acp_turn_workspace_changes.session_id AND t.client_intent_id=acp_turn_workspace_changes.client_intent_id)", [])?;
    Ok(())
}

pub(crate) fn settle_terminal(
    transaction: &Transaction<'_>,
    session: &str,
) -> Result<(), StoreError> {
    let mut statement = transaction.prepare("SELECT w.client_intent_id,w.root_path,w.terminal_json,w.ambiguous,w.payload_bytes,w.root_id FROM acp_turn_workspace_changes w JOIN turns t ON t.session_id=w.session_id AND t.client_intent_id=w.client_intent_id WHERE w.session_id=?1 AND w.result_json IS NULL AND t.terminal_at IS NOT NULL LIMIT 65")?;
    let rows = statement
        .query_map([session], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, bool>(3)?,
                row.get::<_, i64>(4)?,
                row.get::<_, String>(5)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_ACTIVE_BASELINES as usize {
        return Err(StoreError::Corrupt("workspace baseline limit+1"));
    }
    for (key, root, terminal, ambiguous, old_bytes, root_id) in rows {
        let changes = if ambiguous {
            TurnWorkspaceChanges::untracked()
        } else {
            terminal
                .and_then(|json| serde_json::from_str(&json).ok())
                .unwrap_or_else(TurnWorkspaceChanges::incomplete)
        };
        let mut json = serde_json::to_string(&changes)
            .map_err(|_| StoreError::Corrupt("workspace summary encoding"))?;
        if json.len() + root.len() + root_id.len() > MAX_PAYLOAD_BYTES {
            json = serde_json::to_string(&TurnWorkspaceChanges::untracked())
                .map_err(|_| StoreError::Corrupt("workspace summary encoding"))?;
        }
        let bytes = (json.len() + root.len() + root_id.len()) as i64;
        charge(transaction, session, bytes - old_bytes)?;
        transaction.execute("UPDATE acp_turn_workspace_changes SET baseline_json=NULL,terminal_json=NULL,result_json=?3,payload_bytes=?4 WHERE session_id=?1 AND client_intent_id=?2 AND result_json IS NULL", params![session,key,json,bytes])?;
    }
    Ok(())
}

pub(crate) fn load(
    connection: &Connection,
    session: &str,
    key: &str,
    root_id: &str,
) -> Result<TurnWorkspaceChanges, StoreError> {
    let json: Option<String> = connection.query_row("SELECT result_json FROM acp_turn_workspace_changes WHERE session_id=?1 AND client_intent_id=?2 AND root_id=?3 AND result_json IS NOT NULL", params![session,key,root_id], |row| row.get(0)).optional()?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|_| StoreError::Corrupt("workspace summary encoding"))
    })
    .transpose()
    .map(|changes| changes.unwrap_or_else(TurnWorkspaceChanges::untracked))
}

fn charge(connection: &Connection, session: &str, delta: i64) -> Result<(), StoreError> {
    if delta > 0 {
        let current: i64 = connection.query_row(
            "SELECT shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
            [],
            |row| row.get(0),
        )?;
        if current
            .checked_add(delta)
            .is_none_or(|next| next > LIMITS.durable.shared_payload_bytes_global as i64)
        {
            return Err(StoreError::Capacity {
                resource: "workspacePayload",
                limit: LIMITS.durable.shared_payload_bytes_global,
            });
        }
    }
    connection.execute("UPDATE acp_session_usage SET payload_bytes=payload_bytes+?2,revision=revision+1 WHERE session_id=?1", params![session,delta])?;
    connection.execute("UPDATE acp_project_usage SET payload_bytes=payload_bytes+?2,revision=revision+1 WHERE project_id=(SELECT project_id FROM agent_sessions WHERE id=?1)", params![session,delta])?;
    connection.execute("UPDATE acp_store_usage SET shared_payload_bytes=shared_payload_bytes+?1,revision=revision+1 WHERE singleton=1", [delta])?;
    Ok(())
}
