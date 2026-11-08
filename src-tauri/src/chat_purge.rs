//! Permanent removal of one Chat and everything it owns. Temporary side
//! chats and deleted archived Chats share this path, so both release the
//! same rows, files and usage charges.

use rusqlite::{Connection, OptionalExtension, Transaction, params};

use crate::{
    error::{AppError, AppResult},
    persistence::Database,
};

/// The immutable-history guards a Chat purge lifts and restores.
pub(crate) const HISTORY_GUARDS: [&str; 9] = [
    "deny_activity_delete",
    "deny_agent_session_delete",
    "deny_chat_delete",
    "deny_chat_lineage_delete",
    "deny_fact_delete",
    "deny_item_delete",
    "deny_runtime_epoch_delete",
    "deny_server_request_delete",
    "deny_turn_delete",
];
/// Selects one Chat's rows through an index led by (project_id, chat_id).
const BY_OWNER: &str = "project_id=?1 AND chat_id=?2";
/// Selects one Chat's rows through an index led by session_id.
const BY_SESSION: &str = "session_id IN (SELECT id FROM agent_sessions WHERE project_id=?1 AND chat_id=?2) AND project_id=?1 AND chat_id=?2";
/// Selects rows of the Chat's sessions through an index led by session_id,
/// for tables that carry no chat_id.
const BY_CHAT_SESSION: &str =
    "session_id IN (SELECT id FROM agent_sessions WHERE project_id=?1 AND chat_id=?2)";
/// A Chat's rows, deleted in this order while the guards are lifted, each
/// through an index. Item deletes release their own usage through
/// `release_deleted_item_usage`; turn deletes drop workspace changes, which
/// release theirs. Workspace changes no turn owns are deleted next, so their
/// trigger releases them before the remaining session charge is read and the
/// session delete cannot cascade to them and release them a second time.
const CHAT_ROWS: [(&str, &str); 14] = [
    ("acp_child_thread_transcripts", BY_OWNER),
    ("acp_child_thread_bindings", BY_OWNER),
    ("acp_child_thread_owners", BY_OWNER),
    ("acp_turn_usage_snapshots", BY_OWNER),
    ("acp_turn_terminal_reasons", BY_SESSION),
    ("chat_queued_prompts", BY_OWNER),
    ("server_requests", BY_OWNER),
    ("session_facts", BY_OWNER),
    ("session_activities", BY_OWNER),
    ("items", BY_OWNER),
    ("turns", BY_OWNER),
    ("acp_turn_workspace_changes", BY_CHAT_SESSION),
    ("session_runtime_epochs", BY_OWNER),
    ("chat_session_heads", BY_OWNER),
];
/// Image artifact rows leave the purge transaction in bounded batches.
const IMAGE_BATCH: usize = 500;

/// What the Chat's sessions still charge once its rows are gone.
struct SessionCharge {
    sessions: i64,
    turns: i64,
    items: i64,
    item_bytes: i64,
    facts: i64,
    fact_bytes: i64,
    payload_bytes: i64,
}

impl Database {
    /// Deletes the Chat, its history, sessions, tabs and image files, and
    /// releases every Project and store usage charge it held, in one
    /// transaction. `admit` runs before anything changes and again inside
    /// that transaction, so a Chat that stopped qualifying is never removed.
    pub(crate) fn purge_chat(
        &self,
        project: &str,
        chat: &str,
        admit: impl Fn(&Connection) -> AppResult<()>,
    ) -> AppResult<()> {
        admit(&self.connection())?;
        // Refuse before touching anything when a guard is already missing.
        history_guards(&self.connection())?;
        let images = {
            let mut connection = self.write_connection()?;
            let transaction = connection.transaction()?;
            admit(&transaction)?;
            transaction.pragma_update(None, "defer_foreign_keys", true)?;
            let guards = history_guards(&transaction)?;
            for (name, _) in &guards {
                transaction.execute_batch(&format!("DROP TRIGGER \"{name}\""))?;
            }
            let images = take_chat_images(&transaction, project, chat, IMAGE_BATCH)?;
            delete_chat_rows(&transaction, project, chat)?;
            for (_, sql) in guards {
                transaction.execute_batch(&sql)?;
            }
            // foreign_keys is ON and checks are deferred, so COMMIT itself
            // refuses a purge that would leave a dangling reference, without
            // a table scan: AppError::Database(SqliteFailure, extended code
            // 787 FOREIGNKEY). The dropped transaction then rolls back,
            // restoring the lifted guards and the image rows.
            transaction.commit()?;
            images
        };
        // Files go only once their rows are gone for good. A crash before
        // this leaves orphan files that startup reconciliation
        // (image_artifacts::reconcile_image_artifacts) removes.
        self.remove_image_files(&images);
        Ok(())
    }

    /// Unlinks the full and thumbnail files of artifacts whose rows are
    /// already committed as deleted.
    fn remove_image_files(&self, ids: &[String]) {
        let Ok(root) = self.image_artifact_root() else {
            return;
        };
        for id in ids {
            if uuid::Uuid::parse_str(id).is_ok_and(|uuid| uuid.to_string() == *id) {
                for extension in ["full", "thumb"] {
                    let _ = crate::acp::migration::remove_image_file(
                        &root.join(format!("{id}.{extension}")),
                    );
                }
            }
        }
    }
}

/// Deletes a Chat's image artifact rows `batch` at a time inside the purge
/// transaction, each batch through the (project_id, chat_id) index, and
/// returns their ids so the files can go after COMMIT.
pub(crate) fn take_chat_images(
    transaction: &Transaction<'_>,
    project: &str,
    chat: &str,
    batch: usize,
) -> AppResult<Vec<String>> {
    let mut select = transaction.prepare(
        "SELECT id FROM chat_image_artifacts WHERE project_id=?1 AND chat_id=?2 LIMIT ?3",
    )?;
    let mut delete = transaction.prepare("DELETE FROM chat_image_artifacts WHERE id=?1")?;
    let mut taken = Vec::new();
    loop {
        let ids = select
            .query_map(params![project, chat, batch as i64], |row| {
                row.get::<_, String>(0)
            })?
            .collect::<Result<Vec<_>, _>>()?;
        if ids.is_empty() {
            return Ok(taken);
        }
        for id in &ids {
            delete.execute([id])?;
        }
        taken.extend(ids);
    }
}

fn delete_chat_rows(transaction: &Transaction<'_>, project: &str, chat: &str) -> AppResult<()> {
    for (table, scope) in CHAT_ROWS {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE {scope}"),
            params![project, chat],
        )?;
    }
    // A fork's lineage belongs to the fork. A Chat that other forks descend
    // from is refused by their lineage's RESTRICT reference to its turns.
    transaction.execute(
        "DELETE FROM chat_lineage WHERE child_project_id=?1 AND child_chat_id=?2",
        params![project, chat],
    )?;
    let charge = remaining_session_charge(transaction, project, chat)?;
    for (table, scope) in [
        ("acp_session_usage", BY_SESSION),
        ("agent_sessions", BY_OWNER),
    ] {
        transaction.execute(
            &format!("DELETE FROM {table} WHERE {scope}"),
            params![project, chat],
        )?;
    }
    transaction.execute(
        "DELETE FROM chat_tabs WHERE project_id=?1 AND chat_id=?2",
        params![project, chat],
    )?;
    transaction.execute(
        "DELETE FROM workbench_tabs WHERE project_id=?1 AND id=?2",
        params![project, chat],
    )?;
    transaction.execute(
        "DELETE FROM chats WHERE project_id=?1 AND id=?2",
        params![project, chat],
    )?;
    release_usage(transaction, project, &charge)?;
    let fallback: Option<String> = transaction.query_row("SELECT id FROM workbench_tabs WHERE project_id=?1 AND kind='chat' AND id NOT IN (SELECT id FROM workbench_tabs WHERE project_id=?1 AND json_extract(metadata_json,'$.ephemeral')=1) ORDER BY position LIMIT 1", [project], |row| row.get(0)).optional()?;
    transaction.execute(
        "UPDATE project_layouts SET active_tab_id=?3 WHERE project_id=?1 AND active_tab_id=?2",
        params![project, chat, fallback],
    )?;
    Ok(())
}

fn remaining_session_charge(
    transaction: &Transaction<'_>,
    project: &str,
    chat: &str,
) -> AppResult<SessionCharge> {
    Ok(transaction.query_row(
        &format!(
            "SELECT COUNT(*),COALESCE(SUM(turn_count),0),COALESCE(SUM(item_count),0),
                    COALESCE(SUM(item_bytes),0),COALESCE(SUM(fact_count),0),
                    COALESCE(SUM(fact_bytes),0),COALESCE(SUM(payload_bytes),0)
             FROM acp_session_usage WHERE {BY_SESSION}"
        ),
        params![project, chat],
        |row| {
            Ok(SessionCharge {
                sessions: row.get(0)?,
                turns: row.get(1)?,
                items: row.get(2)?,
                item_bytes: row.get(3)?,
                facts: row.get(4)?,
                fact_bytes: row.get(5)?,
                payload_bytes: row.get(6)?,
            })
        },
    )?)
}

/// Releases the Chat and its sessions' remaining charge. The usage CHECK
/// constraints refuse a release that would drive a counter below zero.
fn release_usage(
    transaction: &Transaction<'_>,
    project: &str,
    charge: &SessionCharge,
) -> AppResult<()> {
    let project_rows = transaction.execute(
        "UPDATE acp_project_usage SET chat_count=chat_count-1,session_count=session_count-?2,
           turn_count=turn_count-?3,item_count=item_count-?4,fact_count=fact_count-?5,
           payload_bytes=payload_bytes-?6,revision=revision+1
         WHERE project_id=?1",
        params![
            project,
            charge.sessions,
            charge.turns,
            charge.items,
            charge.facts,
            charge.payload_bytes
        ],
    )?;
    let store_rows = transaction.execute(
        "UPDATE acp_store_usage SET chat_count=chat_count-1,session_count=session_count-?1,
           turn_count=turn_count-?2,item_count=item_count-?3,item_bytes=item_bytes-?4,
           fact_count=fact_count-?5,fact_bytes=fact_bytes-?6,
           shared_payload_bytes=shared_payload_bytes-?7,revision=revision+1
         WHERE singleton=1",
        params![
            charge.sessions,
            charge.turns,
            charge.items,
            charge.item_bytes,
            charge.facts,
            charge.fact_bytes,
            charge.payload_bytes
        ],
    )?;
    if project_rows != 1 || store_rows != 1 {
        return Err(AppError::InvalidRequest(
            "Chat usage ledger is missing; the Chat was not deleted".into(),
        ));
    }
    Ok(())
}

/// Each guard by name, with the SQL that restores it; a missing one names itself.
fn history_guards(connection: &Connection) -> AppResult<Vec<(&'static str, String)>> {
    HISTORY_GUARDS
        .into_iter()
        .map(|name| {
            connection
                .query_row(
                    "SELECT sql FROM sqlite_schema WHERE type='trigger' AND name=?1",
                    [name],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
                .map(|sql| (name, sql))
                .ok_or_else(|| {
                    AppError::InvalidRequest(format!(
                        "immutable history guard {name} is missing; Chat deletion refused"
                    ))
                })
        })
        .collect()
}
