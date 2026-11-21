//! Shared discovery policy. All lookups use the owning chat/session indexes;
//! no transcript payloads or additional rail rows cross into the WebView.
use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::AppResult;
use sage_db::error::SqliteResultExt;

/// The chat aliased `c` has a visible user-prompted Turn. Harness-only and
/// undone Turns never make an otherwise empty chat discoverable.
const PROMPTED_AND_VISIBLE: &str = concat!(
    crate::acp::store::prompted_turn!("listed_turn.session_id", "listed_turn.id"),
    " AND ",
    crate::acp::store::turn_undo::visible_turn!("listed_turn.session_id", "listed_turn.id"),
);

/// Explicit user choices and queued input preserve empty chats as well.
/// A detached durable Chat is archived; that state itself is retention intent,
/// independent of the pin on the tab that was removed when archiving it.
pub(crate) fn listed_chat() -> String {
    let welcome = crate::session_events::legacy_welcome::turn_query("c.id");
    format!(
        "(c.title_source IN ('user','fork')
          OR EXISTS(SELECT 1 FROM chat_lineage lineage
            WHERE lineage.child_project_id=c.project_id AND lineage.child_chat_id=c.id
              AND lineage.relation='nativeFork')
          OR NOT EXISTS(SELECT 1 FROM chat_tabs archived
            WHERE archived.project_id=c.project_id AND archived.chat_id=c.id)
          OR EXISTS(SELECT 1 FROM chat_tabs listed_attachment
          JOIN workbench_tabs listed_tab ON listed_tab.id=listed_attachment.tab_id
          WHERE listed_attachment.project_id=c.project_id AND listed_attachment.chat_id=c.id
            AND listed_tab.pinned=1)
          OR EXISTS(SELECT 1 FROM chat_queued_prompts listed_queue
            WHERE listed_queue.project_id=c.project_id AND listed_queue.chat_id=c.id
              AND json_array_length(listed_queue.prompts_json)>0)
          OR EXISTS(SELECT 1 FROM agent_sessions listed_session JOIN turns listed_turn
            ON listed_turn.session_id=listed_session.id
            WHERE listed_session.project_id=c.project_id AND listed_session.chat_id=c.id
              AND {PROMPTED_AND_VISIBLE}
              AND listed_turn.id<>COALESCE(({welcome}),'')))"
    )
}

/// Only attached, default Chats with no user prompt or retention intent can
/// receive a project draft. The renderer supplies bounded unsent-input owners.
pub(crate) fn reusable_chat(
    connection: &Connection,
    project: &str,
    protected: &[String],
    candidate: Option<&str>,
) -> AppResult<Option<String>> {
    if protected.len() > 128 {
        return Err(sage_core::error::AppError::ResourceLimit(
            "Too many unsent Chat owners".into(),
        ));
    }
    for id in protected {
        super::validate_chat_identifier("Unsent Chat ID", id)?;
    }
    let protected = serde_json::to_string(protected)
        .map_err(|error| sage_core::error::AppError::InvalidRequest(error.to_string()))?;
    connection
        .query_row(
            &format!(
                concat!(
                    "SELECT c.id FROM chats c WHERE c.project_id=?1
            AND c.title_source='default' AND NOT ({}) AND (?3 IS NULL OR c.id=?3)
             AND c.id NOT IN (SELECT value FROM json_each(?2))
             AND NOT EXISTS(SELECT 1 FROM agent_sessions s JOIN turns t ON t.session_id=s.id
               WHERE s.project_id=c.project_id AND s.chat_id=c.id AND ",
                    crate::acp::store::prompted_turn!("t.session_id", "t.id"),
                    " AND t.id<>COALESCE(({}),''))
             AND EXISTS(SELECT 1 FROM chat_tabs a JOIN workbench_tabs w ON w.id=a.tab_id
               WHERE a.project_id=c.project_id AND a.chat_id=c.id
                 AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0)
             ORDER BY c.ordinal,c.id LIMIT 1"
                ),
                listed_chat(),
                crate::session_events::legacy_welcome::turn_query("c.id")
            ),
            params![project, protected, candidate],
            |row| row.get(0),
        )
        .optional()
        .db()
}

/// A project boundary releases an empty selection, never its chat or files.
/// Preserve a selected resource or discoverable chat; otherwise choose the
/// newest discoverable attached chat. NULL opens the unsent draft.
pub(crate) fn reopen_project(connection: &Connection, project: &str) -> AppResult<()> {
    let listed = listed_chat();
    connection
        .execute(
            &format!(
                "UPDATE project_layouts SET active_tab_id=(
              SELECT w.id FROM workbench_tabs w
              JOIN chat_tabs ct ON ct.project_id=w.project_id AND ct.tab_id=w.id
              JOIN chats c ON c.project_id=ct.project_id AND c.id=ct.chat_id
              WHERE w.project_id=?1 AND {listed}
                AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0
              ORDER BY c.updated_at DESC,c.ordinal DESC,c.id LIMIT 1)
             WHERE project_id=?1 AND (active_tab_id IS NULL OR EXISTS(
               SELECT 1 FROM workbench_tabs w
               LEFT JOIN chat_tabs ct ON ct.project_id=w.project_id AND ct.tab_id=w.id
               LEFT JOIN chats c ON c.project_id=ct.project_id AND c.id=ct.chat_id
               WHERE w.project_id=?1 AND w.id=project_layouts.active_tab_id
                 AND w.kind='chat' AND NOT {listed}))"
            ),
            params![project],
        )
        .db()?;
    Ok(())
}

#[cfg(test)]
mod tests;
