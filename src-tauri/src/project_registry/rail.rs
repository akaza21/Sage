use rusqlite::{Connection, params};

use crate::domain::{AgentRailChat, ProjectListItem};
use crate::error::AppResult;

use super::MAX_PROJECTS;

pub(super) const MAX_AGENT_RAIL_CHATS_PER_PROJECT: i64 = 8;

pub(super) fn load_project_list(connection: &Connection) -> AppResult<Vec<ProjectListItem>> {
    let mut statement = connection.prepare(
        "SELECT p.id, p.name, COUNT(r.id), p.last_opened_at
         FROM projects p
         LEFT JOIN project_roots r ON r.project_id = p.id
         GROUP BY p.id
         ORDER BY p.last_opened_at DESC, p.name
         LIMIT ?1",
    )?;
    let mut projects = statement
        .query_map([MAX_PROJECTS + 1], |row| {
            Ok(ProjectListItem {
                id: row.get(0)?,
                name: row.get(1)?,
                root_count: row.get::<_, i64>(2)? as u32,
                last_opened_at: row.get(3)?,
                rail_chats: Vec::new(),
                rail_chats_truncated: false,
                rail_archived: Vec::new(),
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    attach_rail_chats(connection, &mut projects)?;
    attach_rail_archived(connection, &mut projects)?;
    Ok(projects)
}

type RailChatRow = (
    String,
    String,
    bool,
    i64,
    Option<String>,
    Option<i64>,
    Option<i64>,
);

/// The finish time of the chat aliased `c`'s latest Turn, from its latest
/// session that has one; NULL while that Turn runs or the chat has none.
pub(crate) const CHAT_FINISHED_AT: &str = "(SELECT t.terminal_at FROM turns t
           WHERE t.session_id = (SELECT s.id FROM agent_sessions s
             WHERE s.project_id = c.project_id AND s.chat_id = c.id
               AND EXISTS(SELECT 1 FROM turns present WHERE present.session_id = s.id)
             ORDER BY s.ordinal DESC, s.id DESC LIMIT 1)
           ORDER BY t.ordinal DESC, t.id DESC LIMIT 1)";

fn rail_chat_columns() -> String {
    format!(
        "SELECT w.id, w.title, w.pinned, w.position, w.status,
                COALESCE(c.updated_at, w.updated_at), {CHAT_FINISHED_AT}
         FROM workbench_tabs w
         LEFT JOIN chat_tabs ct ON ct.project_id = w.project_id AND ct.tab_id = w.id
         LEFT JOIN chats c ON c.project_id = ct.project_id AND c.id = ct.chat_id
         WHERE w.project_id = ?1 AND w.kind = 'chat' AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0"
    )
}

/// Each project's first chats in both orders the sidebar offers (last updated
/// and manual), so either order previews the right chats without a refetch.
/// At most twice the per-project cap; `rail_chats_truncated` marks the rest.
pub(super) fn attach_rail_chats(
    connection: &Connection,
    projects: &mut [ProjectListItem],
) -> AppResult<()> {
    let mut by_recency = connection.prepare(&format!(
        "{}
         ORDER BY w.pinned DESC, COALESCE(c.updated_at, w.updated_at, 0) DESC, w.position, w.id
         LIMIT ?2",
        rail_chat_columns()
    ))?;
    let mut by_position = connection.prepare(&format!(
        "{}
         ORDER BY w.pinned DESC, w.position, w.id
         LIMIT ?2",
        rail_chat_columns()
    ))?;
    let mut total = connection.prepare(
        "SELECT COUNT(*) FROM workbench_tabs w
         WHERE w.project_id = ?1 AND w.kind = 'chat' AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0",
    )?;
    let read = |row: &rusqlite::Row<'_>| -> rusqlite::Result<RailChatRow> {
        Ok((
            row.get(0)?,
            row.get(1)?,
            row.get::<_, i64>(2)? != 0,
            row.get(3)?,
            row.get(4)?,
            row.get(5)?,
            row.get(6)?,
        ))
    };
    for project in projects.iter_mut() {
        if project.id.is_empty() || project.id.len() > 512 {
            continue;
        }
        let mut rows = by_recency
            .query_map(params![project.id, MAX_AGENT_RAIL_CHATS_PER_PROJECT], read)?
            .collect::<Result<Vec<_>, _>>()?;
        for row in
            by_position.query_map(params![project.id, MAX_AGENT_RAIL_CHATS_PER_PROJECT], read)?
        {
            let row = row?;
            if !rows.iter().any(|held| held.0 == row.0) {
                rows.push(row);
            }
        }
        let count: i64 = total.query_row(params![project.id], |row| row.get(0))?;
        project.rail_chats_truncated = count > rows.len() as i64;
        project.rail_chats = rows
            .into_iter()
            .filter(|(id, title, ..)| !id.is_empty() && id.len() <= 512 && !title.contains('\0'))
            .map(
                |(id, title, pinned, position, status, updated_at, finished_at)| AgentRailChat {
                    id,
                    title,
                    pinned,
                    position,
                    status,
                    updated_at,
                    finished_at,
                },
            )
            .collect();
    }
    Ok(())
}

fn attach_rail_archived(
    connection: &Connection,
    projects: &mut [ProjectListItem],
) -> AppResult<()> {
    for project in projects.iter_mut() {
        if project.id.is_empty() || project.id.len() > 512 {
            continue;
        }
        project.rail_archived = crate::chat_archive::archived_chats_in(connection, &project.id)?;
    }
    Ok(())
}
