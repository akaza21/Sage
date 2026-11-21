//! Shared discovery policy. All lookups use the owning chat/session indexes;
//! no transcript payloads or additional rail rows cross into the WebView.
use rusqlite::{Connection, params};
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
pub(crate) fn listed_chat() -> String {
    let welcome = crate::session_events::legacy_welcome::turn_query("c.id");
    format!(
        "(c.title_source='user' OR EXISTS(SELECT 1 FROM chat_tabs listed_attachment
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
