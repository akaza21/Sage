use rusqlite::{OptionalExtension, params};

use crate::acp::{
    contract::{
        BoundedVec, EntityRevision, ProjectionRevision, RuntimeEpoch, SafeText, SageAgentSessionId,
        SageChatId, SageProjectId,
    },
    intent::{BindingId, CapabilitySnapshotV1, ModelId, PageCursor},
    lifecycle::Residency,
    limits::LIMITS,
    projection::{AcpChatPage, SessionProjection},
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

pub(crate) fn load_chat_page(
    connection: &rusqlite::Connection,
    project_id: SageProjectId,
    chat_id: SageChatId,
    cursor: Option<PageCursor>,
) -> Result<AcpChatPage, StoreError> {
    let snapshot = connection.unchecked_transaction()?;
    let page = load_chat_page_in(&snapshot, project_id, chat_id, cursor)?;
    snapshot.commit()?;
    Ok(page)
}

fn load_chat_page_in(
    connection: &rusqlite::Connection,
    project_id: SageProjectId,
    chat_id: SageChatId,
    cursor: Option<PageCursor>,
) -> Result<AcpChatPage, StoreError> {
    let (title, projection, attention): (String, i64, i64) = connection
        .query_row(
            "SELECT c.title,c.projection_revision,COALESCE(a.activity_position,0)
             FROM chats c LEFT JOIN chat_attention a ON a.chat_id=c.id
             WHERE c.project_id=?1 AND c.id=?2",
            params![project_id.as_str(), chat_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::Conflict { resource: "chat" })?;
    let observed_attention_position = cursor
        .is_none()
        .then(|| crate::chat_attention::store::position(attention))
        .transpose()?;
    let sessions = load_sessions(connection, &project_id, &chat_id)?;
    let project_roots = super::history_roots::load_project_roots(connection, &project_id)?;
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
    let empty_page = AcpChatPage {
        handoffs: super::context_handoff::summaries(
            connection,
            project_id.as_str(),
            chat_id.as_str(),
        )?,
        observed_attention_position,
        session_config: None,
        goal: crate::acp::goal::GoalState::Unsupported,
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
    };
    let base_page_bytes = serde_json::to_vec(&empty_page)
        .map_err(|_| StoreError::Corrupt("chat page serialization"))?
        .len();
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
        handoffs: empty_page.handoffs,
        observed_attention_position,
        session_config: None,
        goal: crate::acp::goal::GoalState::Unsupported,
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
    };
    page.validate_page_bounds()
        .map_err(|_| StoreError::Corrupt("chat page ownership"))?;
    let page_bytes = serde_json::to_vec(&page)
        .map_err(|_| StoreError::Corrupt("chat page serialization"))?
        .len();
    if page_bytes > LIMITS.durable.history_page_bytes as usize {
        return Err(StoreError::Capacity {
            resource: "historyPage",
            limit: LIMITS.durable.history_page_bytes,
        });
    }
    Ok(page)
}

fn load_sessions(
    connection: &rusqlite::Connection,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<Vec<SessionProjection>, StoreError> {
    let mut statement=connection.prepare("SELECT s.id,s.ordinal,s.binding_id,s.model_id,s.state,s.residency,(SELECT MAX(e.runtime_epoch) FROM session_runtime_epochs e WHERE e.session_id=s.id AND e.state IN ('starting','running','terminating')),(SELECT MAX(e.runtime_epoch) FROM session_runtime_epochs e WHERE e.session_id=s.id),s.capabilities_json,s.revision,s.projection_revision,s.legacy FROM agent_sessions s WHERE s.project_id=?1 AND s.chat_id=?2 ORDER BY s.ordinal,id LIMIT 33")?;
    let rows = statement.query_map(params![project.as_str(), chat.as_str()], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, i64>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, Option<String>>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<i64>>(7)?,
            row.get::<_, String>(8)?,
            row.get::<_, i64>(9)?,
            row.get::<_, i64>(10)?,
            row.get::<_, i64>(11)?,
        ))
    })?;
    let mut result = Vec::with_capacity(32);
    for row in rows {
        if result.len() == 32 {
            return Err(StoreError::Corrupt("session page limit+1"));
        }
        let row = row?;
        let state = super::state::parse_session_state(&row.4)?;
        let residency = match row.5.as_str() {
            "hot" => Residency::Hot,
            "cold" => Residency::Cold,
            _ => return Err(StoreError::Corrupt("residency")),
        };
        let runtime_epoch = row
            .6
            .map(|value| {
                RuntimeEpoch::new(
                    u64::try_from(value).map_err(|_| StoreError::Corrupt("runtime epoch"))?,
                )
                .map_err(|_| StoreError::Corrupt("runtime epoch"))
            })
            .transpose()?;
        let next_runtime_epoch = if runtime_epoch.is_none()
            && residency == Residency::Cold
            && matches!(
                state,
                crate::acp::lifecycle::SessionState::Created
                    | crate::acp::lifecycle::SessionState::Ready
            ) {
            let latest = row.7.unwrap_or(0);
            let next = latest.checked_add(1).ok_or(StoreError::CounterExhausted)?;
            Some(
                RuntimeEpoch::new(
                    u64::try_from(next).map_err(|_| StoreError::Corrupt("runtime epoch"))?,
                )
                .map_err(|_| StoreError::Corrupt("runtime epoch"))?,
            )
        } else {
            None
        };
        let binding_id =
            BindingId::new("bindingId", row.2).map_err(|_| StoreError::Corrupt("binding id"))?;
        let capabilities = serde_json::from_str::<CapabilitySnapshotV1>(&row.8)
            .map_err(|_| StoreError::Corrupt("capabilities"))?;
        result.push(SessionProjection {
            id: SageAgentSessionId::parse(row.0).map_err(|_| StoreError::Corrupt("session id"))?,
            ordinal: u64::try_from(row.1).map_err(|_| StoreError::Corrupt("session ordinal"))?,
            binding_id: binding_id.clone(),
            model_id: row
                .3
                .map(|v| ModelId::new("modelId", v))
                .transpose()
                .map_err(|_| StoreError::Corrupt("model id"))?,
            state,
            residency,
            runtime_epoch,
            next_runtime_epoch,
            permission_requests: permission_requests_supported(&binding_id, &capabilities),
            capabilities,
            revision: EntityRevision::new(row.9 as u64)
                .map_err(|_| StoreError::Corrupt("session revision"))?,
            projection_revision: ProjectionRevision::new(row.10 as u64)
                .map_err(|_| StoreError::Corrupt("projection revision"))?,
            legacy: match row.11 {
                0 => false,
                1 => true,
                _ => return Err(StoreError::Corrupt("legacy flag")),
            },
        });
    }
    Ok(result)
}

fn permission_requests_supported(
    binding_id: &BindingId,
    capabilities: &CapabilitySnapshotV1,
) -> bool {
    match binding_id.as_str() {
        "codex-app-server" => {
            crate::acp::bindings::codex::supports_native_0146_protocol(
                capabilities.protocol_version.as_str(),
            ) && capabilities.active_turn_steer
                && crate::acp::bindings::access_policy::CODEX_ASK_NATIVE_APPROVAL_PROVEN
        }
        "claude-agent-acp" | "cursor-agent-acp" | "opencode-acp" | "gemini-cli-acp" => true,
        _ => false,
    }
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

#[cfg(test)]
mod permission_request_tests {
    use super::*;

    fn capabilities(protocol_version: &str) -> CapabilitySnapshotV1 {
        serde_json::from_value(serde_json::json!({
            "schemaVersion": 1,
            "protocolVersion": protocol_version,
            "newSession": true,
            "resumeSession": true,
            "loadSession": true,
            "closeSession": true,
            "prompt": true,
            "cancel": true,
            "nativeSessionFork": false,
            "activeTurnSteer": false,
            "childConversationProjection": false,
            "childTranscriptRead": false,
            "collaborationEventProjection": false,
            "multiRootExtensionSchemaId": null,
            "modelCount": 0,
            "configCount": 0,
        }))
        .expect("capabilities")
    }

    #[test]
    fn codex_ask_projection_requires_native_approval_evidence() {
        let codex = BindingId::new("bindingId", "codex-app-server").expect("Codex binding");
        assert!(!permission_requests_supported(
            &codex,
            &capabilities("codex-app-server-0.145.0-7b0ab167")
        ));
        let mut managed = capabilities("codex-app-server-0.146.0-d3992fec");
        managed.active_turn_steer = true;
        assert!(!permission_requests_supported(&codex, &managed));
        let mut observed = capabilities("codex-app-server-observed-0.146.0");
        assert!(!permission_requests_supported(&codex, &observed));
        observed.active_turn_steer = true;
        let advertised =
            crate::acp::bindings::access_policy::access_modes("codex-app-server", "0.146.0", true)
                .iter()
                .any(|mode| mode.id == crate::settings_store::SessionAccessMode::Ask);
        assert_eq!(permission_requests_supported(&codex, &observed), advertised);

        let claude = BindingId::new("bindingId", "claude-agent-acp").expect("Claude binding");
        assert!(permission_requests_supported(&claude, &capabilities("1")));

        let unknown = BindingId::new("bindingId", "fixture").expect("fixture binding");
        assert!(!permission_requests_supported(
            &unknown,
            &capabilities("fixture-1")
        ));
    }
}
