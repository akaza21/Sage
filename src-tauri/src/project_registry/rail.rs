use rusqlite::{Connection, params};
use sage_core::error::AppResult;

use sage_domain::project::{AgentRailChat, ProjectListItem};

use super::MAX_PROJECTS;

/// The chats the rail previews per project in each order it offers.
pub(crate) const MAX_AGENT_RAIL_CHATS_PER_PROJECT: i64 = 8;

/// The rail's "last updated" order over tabs aliased `w` and their chats
/// aliased `c`: pinned chats first, then the most recently updated.
const RAIL_RECENCY_ORDER: &str =
    "w.pinned DESC, COALESCE(c.updated_at, w.updated_at, 0) DESC, w.position, w.id";

/// The rail's manual order: pinned chats first, then by tab position.
const RAIL_POSITION_ORDER: &str = "w.pinned DESC, w.position, w.id";

/// The chat tabs the rail lists, as tabs aliased `w` with their chats
/// aliased `c` (absent for a tab whose chat is not attached).
const RAIL_CHAT_TABS: &str = "FROM workbench_tabs w
         LEFT JOIN chat_tabs ct ON ct.project_id = w.project_id AND ct.tab_id = w.id
         LEFT JOIN chats c ON c.project_id = ct.project_id AND c.id = ct.chat_id
         WHERE w.kind = 'chat' AND COALESCE(json_extract(w.metadata_json,'$.ephemeral'),0)=0";

/// The most chats the rail can show at once: every chat tab of the open
/// project, and each other project's preview in both orders.
pub(crate) const MAX_RAIL_CHATS: usize = crate::workbench_restore::MAX_RESTORED_TABS
    + (MAX_PROJECTS as usize - 1) * 2 * MAX_AGENT_RAIL_CHATS_PER_PROJECT as usize;

/// Every chat the rail can show, as a derived table with `project_id`,
/// `tab_id` and `id` (the attached chat's id, or NULL): all chat tabs of the
/// active project, and each other project's first chats in both orders
/// `attach_rail_chats` previews. At most `MAX_RAIL_CHATS` rows. Ranking reads
/// only tab and chat rows, so callers can look up Turns for these rows alone.
pub(crate) fn rail_chat_set() -> String {
    format!(
        "(SELECT ranked.project_id AS project_id, ranked.tab_id AS tab_id, ranked.id AS id
          FROM (
            SELECT w.project_id AS project_id, w.id AS tab_id, c.id AS id,
                   ROW_NUMBER() OVER (PARTITION BY w.project_id ORDER BY {RAIL_RECENCY_ORDER}) AS by_recency,
                   ROW_NUMBER() OVER (PARTITION BY w.project_id ORDER BY {RAIL_POSITION_ORDER}) AS by_position
            {RAIL_CHAT_TABS}
          ) ranked
          WHERE ranked.project_id IS (SELECT active_project_id FROM app_state WHERE singleton = 1)
             OR ranked.by_recency <= {MAX_AGENT_RAIL_CHATS_PER_PROJECT}
             OR ranked.by_position <= {MAX_AGENT_RAIL_CHATS_PER_PROJECT})"
    )
}

/// The tab ids of every chat the rail can show; see `rail_chat_set`.
pub(crate) fn rail_chat_ids(connection: &Connection) -> AppResult<Vec<String>> {
    let mut statement = connection.prepare(&format!("SELECT tab_id FROM {}", rail_chat_set()))?;
    let ids = statement
        .query_map([], |row| row.get(0))?
        .collect::<Result<Vec<String>, _>>()?;
    Ok(ids)
}

/// Projects newest first by when they were added. Opening a project or one of
/// its chats never reorders the list, so rows stay put under the pointer.
pub(super) fn load_project_list(connection: &Connection) -> AppResult<Vec<ProjectListItem>> {
    let mut statement = connection.prepare(
        "SELECT p.id, p.name, COUNT(r.id), p.last_opened_at
         FROM projects p
         LEFT JOIN project_roots r ON r.project_id = p.id
         GROUP BY p.id
         ORDER BY p.created_at DESC, p.id
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

/// A column of the chat aliased `c`'s latest visible Turn, from its latest
/// session that has one. Every read of "the chat's latest Turn" goes through
/// this, so the rail and the attention query agree on which Turn that is.
macro_rules! chat_latest_turn {
    ($column:literal) => {
        concat!(
            "(SELECT t.",
            $column,
            " FROM turns t
           WHERE t.session_id = (SELECT s.id FROM agent_sessions s
             WHERE s.project_id = c.project_id AND s.chat_id = c.id
               AND EXISTS(SELECT 1 FROM turns present WHERE present.session_id = s.id AND ",
            crate::acp::store::turn_undo::visible_turn!("present.session_id", "present.id"),
            ")
             ORDER BY s.ordinal DESC, s.id DESC LIMIT 1) AND ",
            crate::acp::store::turn_undo::visible_turn!("t.session_id", "t.id"),
            "
           ORDER BY t.ordinal DESC, t.id DESC LIMIT 1)"
        )
    };
}

/// The finish time of the chat aliased `c`'s latest Turn; NULL while that
/// Turn runs or the chat has none.
pub(crate) const CHAT_FINISHED_AT: &str = chat_latest_turn!("terminal_at");

/// The id of the chat aliased `c`'s latest Turn; NULL when it has none.
pub(crate) const CHAT_LATEST_TURN_ID: &str = chat_latest_turn!("id");

fn rail_chat_columns() -> String {
    format!(
        "SELECT w.id, w.title, w.pinned, w.position, w.status,
                COALESCE(c.updated_at, w.updated_at), {CHAT_FINISHED_AT}
         {RAIL_CHAT_TABS} AND w.project_id = ?1"
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
         ORDER BY {RAIL_RECENCY_ORDER}
         LIMIT ?2",
        rail_chat_columns()
    ))?;
    let mut by_position = connection.prepare(&format!(
        "{}
         ORDER BY {RAIL_POSITION_ORDER}
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
