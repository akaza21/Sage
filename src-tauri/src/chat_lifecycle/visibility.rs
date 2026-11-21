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
/// Archive/restore intent survives attachment changes independently of pins.
pub(crate) fn listed_chat() -> String {
    let welcome = crate::session_events::legacy_welcome::turn_query("c.id");
    format!(
        "(c.title_source IN ('user','fork')
          OR EXISTS(SELECT 1 FROM chat_lineage lineage
            WHERE lineage.child_project_id=c.project_id AND lineage.child_chat_id=c.id
              AND lineage.relation='nativeFork')
          OR c.retained_at IS NOT NULL
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

/// At startup there is no renderer-owned unsent input to protect. Release
/// hidden attachments across all projects before loading any WebView state.
/// Live project switches keep open chats and their unsent input attached.
pub(crate) fn release_hidden_tabs(connection: &Connection) -> AppResult<()> {
    let hidden = format!(
        "SELECT a.tab_id FROM chat_tabs a JOIN chats c
           ON c.project_id=a.project_id AND c.id=a.chat_id
         JOIN workbench_tabs w ON w.id=a.tab_id
         WHERE NOT ({})
           AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0
           AND NOT EXISTS(SELECT 1 FROM workbench_tabs side
             WHERE side.project_id=c.project_id
               AND json_extract(side.metadata_json,'$.ephemeral')=1
               AND json_extract(side.metadata_json,'$.parentChatId')=c.id)",
        listed_chat(),
    );
    let transaction = connection.unchecked_transaction().db()?;
    transaction
        .execute(
            &format!(
                "UPDATE project_layouts SET active_tab_id=NULL WHERE active_tab_id IN ({hidden})"
            ),
            [],
        )
        .db()?;
    // Cascading removes the attachment, never the chat or its history.
    transaction
        .execute(
            &format!("DELETE FROM workbench_tabs WHERE id IN ({hidden})"),
            [],
        )
        .db()?;
    transaction.commit().db()
}

#[cfg(test)]
mod tests;
