use rusqlite::{OptionalExtension, params};

use crate::acp::{
    contract::{
        BoundedVec, ProjectionRevision, SafeText, SageAgentSessionId, SageChatId, SageProjectId,
        SageRootId,
    },
    intent::PageCursor,
    limits::LIMITS,
    projection::AcpChatPage,
};

use super::StoreError;

pub(super) struct TurnRow {
    pub(super) id: String,
    pub(super) project_id: String,
    pub(super) chat_id: String,
    pub(super) session_id: String,
    pub(super) session_ordinal: i64,
    pub(super) runtime_epoch: i64,
    pub(super) ordinal: i64,
    pub(super) state: String,
    pub(super) revision: i64,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
    pub(super) terminal_at: Option<i64>,
    pub(super) item_before: Option<i64>,
}

/// A page whose history leaves `reserve` bytes of the page budget free for
/// what the service adds to it.
pub(crate) fn load_chat_page_reserving(
    connection: &rusqlite::Connection,
    project_id: SageProjectId,
    chat_id: SageChatId,
    cursor: Option<PageCursor>,
    reserve: usize,
) -> Result<AcpChatPage, StoreError> {
    let (title, projection): (String, i64) = connection
        .query_row(
            "SELECT title,projection_revision FROM chats WHERE project_id=?1 AND id=?2",
            params![project_id.as_str(), chat_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    let sessions = super::history_sessions::load_sessions(connection, &project_id, &chat_id)?;
    let project_roots = load_project_roots(connection, &project_id)?;
    let current = connection
        .query_row(
            "SELECT session_id FROM chat_session_heads WHERE project_id=?1 AND chat_id=?2",
            params![project_id.as_str(), chat_id.as_str()],
            |row| row.get::<_, String>(0),
        )
        .optional()?
        .map(SageAgentSessionId::parse)
        .transpose()
        .map_err(|_| StoreError::Corrupt("head session id"))?;
    let cursor = cursor
        .map(|cursor| super::history_cursor::resolve(connection, &project_id, &chat_id, cursor))
        .transpose()?;
    let (mut turn_rows, mut has_more) =
        load_turn_rows(connection, &project_id, &chat_id, cursor.as_ref())?;
    super::history_activities::retain_bounded_turn_prefix(
        connection,
        &mut turn_rows,
        &mut has_more,
    )?;
    let activities = super::history_activities::load_activities(connection, &turn_rows)?;
    let title = SafeText::new("chatTitle", title).map_err(|_| StoreError::Corrupt("chat title"))?;
    let projection_revision = ProjectionRevision::new(
        u64::try_from(projection).map_err(|_| StoreError::Corrupt("projection revision"))?,
    )
    .map_err(|_| StoreError::Corrupt("projection revision"))?;
    let sessions = BoundedVec::new("sessions", sessions)
        .map_err(|_| StoreError::Corrupt("session page bound"))?;
    let (forked_from, forked_from_turn_id) =
        super::chat_fork::forked_from(connection, &project_id, &chat_id)?.unzip();
    let child_projection_epoch = super::child_transcript::child_projection_epoch(
        connection,
        project_id.as_str(),
        chat_id.as_str(),
    )?;
    let session_notices = super::session_surface::notices(
        connection,
        project_id.as_str(),
        chat_id.as_str(),
        current.as_ref().map(SageAgentSessionId::as_str),
    )?;
    let empty_page = AcpChatPage {
        session_config: None,
        restored_session_config: None,
        session_config_notice: None,
        goal: crate::acp::goal::GoalState::Unsupported,
        // Counted as set: the service sets it after the history is sized.
        usage: true,
        session_surface: None,
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        title: title.clone(),
        projection_revision,
        current_session_id: current.clone(),
        sessions: sessions.clone(),
        turns: BoundedVec::new("turns", Vec::new())
            .map_err(|_| StoreError::Corrupt("turn page bound"))?,
        next_cursor: None,
        forked_from: forked_from.clone(),
        forked_from_turn_id: forked_from_turn_id.clone(),
        child_projection_epoch,
        session_notices: session_notices.clone(),
    };
    let base_page_bytes = crate::acp::encoded_len::encoded_len(&empty_page)
        .map_err(|_| StoreError::Corrupt("chat page serialization"))?
        .saturating_add(reserve);
    let slice = super::history_items::load(super::history_items::HistoryLoad {
        connection,
        project: &project_id,
        chat: &chat_id,
        rows: &turn_rows,
        has_more_turns: has_more,
        project_roots: &project_roots,
        activities,
        base_page_bytes,
    })?;
    let page = AcpChatPage {
        session_config: None,
        restored_session_config: None,
        session_config_notice: None,
        goal: crate::acp::goal::GoalState::Unsupported,
        usage: false,
        session_surface: None,
        project_id,
        chat_id,
        title,
        projection_revision,
        current_session_id: current,
        sessions,
        turns: BoundedVec::new("turns", slice.turns)
            .map_err(|_| StoreError::Corrupt("turn page bound"))?,
        next_cursor: slice.next_cursor,
        forked_from,
        forked_from_turn_id,
        child_projection_epoch,
        session_notices,
    };
    page.validate_page_bounds()
        .map_err(|_| StoreError::Corrupt("chat page ownership"))?;
    let page_bytes = crate::acp::encoded_len::encoded_len(&page)
        .map_err(|_| StoreError::Corrupt("chat page serialization"))?;
    if page_bytes > LIMITS.durable.history_page_bytes as usize {
        return Err(StoreError::Capacity {
            resource: "historyPage",
            limit: LIMITS.durable.history_page_bytes,
        });
    }
    Ok(page)
}

fn load_project_roots(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
) -> Result<Vec<SageRootId>, StoreError> {
    let mut statement = connection
        .prepare("SELECT id FROM project_roots WHERE project_id=?1 ORDER BY ordinal,id LIMIT 9")?;
    let mut roots = Vec::with_capacity(8);
    for row in statement.query_map([project.as_str()], |row| row.get::<_, String>(0))? {
        if roots.len() == 8 {
            return Err(StoreError::Corrupt("project root limit+1"));
        }
        roots.push(SageRootId::parse(row?).map_err(|_| StoreError::Corrupt("project root id"))?);
    }
    // Rootless Projects are valid. Item decoding still checks each referenced
    // resource against this set, so an empty set admits no file resources.
    Ok(roots)
}

fn load_turn_rows(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    cursor: Option<&super::history_cursor::Boundary>,
) -> Result<(Vec<TurnRow>, bool), StoreError> {
    let welcome = crate::session_events::legacy_welcome::turn_id(connection, chat.as_str())?
        .unwrap_or_default();
    let base = "SELECT t.id,t.project_id,t.chat_id,t.session_id,s.ordinal,
                       t.runtime_epoch,t.ordinal,t.state,t.revision,
                       t.created_at,t.updated_at,t.terminal_at
                FROM turns t JOIN agent_sessions s ON s.id=t.session_id
                WHERE t.project_id=?1 AND t.chat_id=?2 AND t.id<>?3";
    let sql = match cursor {
        Some(boundary) if boundary.includes_boundary_turn() => format!(
            "{base} AND (s.ordinal<?4 OR (s.ordinal=?4 AND
             (t.ordinal<?5 OR (t.ordinal=?5 AND t.id<=?6))))
             ORDER BY s.ordinal DESC,t.ordinal DESC,t.id DESC LIMIT 21"
        ),
        Some(_) => format!(
            "{base} AND (s.ordinal<?4 OR (s.ordinal=?4 AND
             (t.ordinal<?5 OR (t.ordinal=?5 AND t.id<?6))))
             ORDER BY s.ordinal DESC,t.ordinal DESC,t.id DESC LIMIT 21"
        ),
        None => format!("{base} ORDER BY s.ordinal DESC,t.ordinal DESC,t.id DESC LIMIT 21"),
    };
    let mut statement = connection.prepare(&sql)?;
    let mapper = |row: &rusqlite::Row<'_>| {
        Ok(TurnRow {
            id: row.get(0)?,
            project_id: row.get(1)?,
            chat_id: row.get(2)?,
            session_id: row.get(3)?,
            session_ordinal: row.get(4)?,
            runtime_epoch: row.get(5)?,
            ordinal: row.get(6)?,
            state: row.get(7)?,
            revision: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
            terminal_at: row.get(11)?,
            item_before: None,
        })
    };
    let mut result = Vec::with_capacity(21);
    if let Some(boundary) = cursor {
        for row in statement.query_map(
            params![
                project.as_str(),
                chat.as_str(),
                welcome,
                boundary.session_ordinal,
                boundary.turn_ordinal,
                boundary.turn_id
            ],
            mapper,
        )? {
            result.push(row?);
        }
    } else {
        for row in statement.query_map(params![project.as_str(), chat.as_str(), welcome], mapper)? {
            result.push(row?);
        }
    }
    let more = result.len() > LIMITS.durable.history_page_turns as usize;
    if more {
        result.pop();
    }
    if let Some(boundary) = cursor.filter(|boundary| boundary.includes_boundary_turn()) {
        let first = result
            .first_mut()
            .filter(|row| row.id == boundary.turn_id)
            .ok_or(StoreError::Conflict {
                resource: "pageCursor",
            })?;
        first.item_before = boundary.item_before;
    }
    Ok((result, more))
}
