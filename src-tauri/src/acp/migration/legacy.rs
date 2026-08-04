use std::collections::HashMap;

use rusqlite::{Transaction, params};

use crate::error::{AppError, AppResult};

use super::{
    identity::legacy_binding_digest,
    legacy_events::convert_session_events,
    limits::{BYTES_PER_PAGE, ROWS_PER_PAGE},
};

const FINAL_SCHEMA: &str = include_str!("../../../migrations/005_acp_session_foundation.sql");
const LEGACY_CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"legacy\",\"newSession\":false,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":false,\"prompt\":false,\"cancel\":false,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":0,\"configCount\":0}";

#[derive(Debug)]
struct LegacyTab {
    id: String,
    project_id: String,
    title: String,
    position: i64,
    created_at: i64,
    updated_at: i64,
}

#[derive(Debug)]
pub(super) struct LegacySession {
    pub(super) id: String,
    pub(super) project_id: String,
    pub(super) chat_tab_id: String,
    pub(super) binding_id: String,
    pub(super) harness_version: Option<String>,
    pub(super) model_id: Option<String>,
    pub(super) external_session_id: Option<String>,
    pub(super) last_sequence: i64,
    pub(super) created_at: i64,
    pub(super) updated_at: i64,
}

pub(super) fn convert(transaction: &Transaction<'_>, applied_at: i64) -> AppResult<()> {
    transaction.execute_batch(
        "ALTER TABLE session_events RENAME TO legacy_session_events_v4;
         ALTER TABLE agent_sessions RENAME TO legacy_agent_sessions_v4;",
    )?;
    transaction.execute_batch(FINAL_SCHEMA)?;
    let tab_projects = convert_tabs(transaction)?;
    convert_sessions(transaction, &tab_projects)?;
    rebuild_and_verify_ledgers(transaction)?;
    transaction.execute_batch(
        "DROP TABLE legacy_session_events_v4;
         DROP TABLE legacy_agent_sessions_v4;",
    )?;
    transaction.execute(
        "INSERT INTO schema_migrations(version, applied_at) VALUES (5, ?1)",
        [applied_at],
    )?;
    transaction.pragma_update(None, "user_version", 5)?;
    Ok(())
}

fn convert_tabs(transaction: &Transaction<'_>) -> AppResult<HashMap<String, String>> {
    let mut owners = HashMap::new();
    let mut ordinals = HashMap::<String, i64>::new();
    let mut cursor: Option<(String, i64, i64, String)> = None;
    loop {
        let page = tab_page(transaction, cursor.as_ref())?;
        if page.is_empty() {
            break;
        }
        for tab in page {
            validate_text("legacy Chat id", &tab.id, 1, 512)?;
            validate_text("legacy Chat title", &tab.title, 1, 1_024)?;
            let ordinal = ordinals.entry(tab.project_id.clone()).or_default();
            transaction.execute(
                "INSERT INTO chats
                 (id, project_id, ordinal, title, projection_revision, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 0, ?5, ?6)",
                params![
                    tab.id,
                    tab.project_id,
                    *ordinal,
                    tab.title,
                    tab.created_at,
                    tab.updated_at
                ],
            )?;
            transaction.execute(
                "INSERT INTO chat_tabs(project_id, chat_id, tab_id, attached_at)
                 VALUES (?1, ?2, ?2, ?3)",
                params![tab.project_id, tab.id, tab.updated_at],
            )?;
            *ordinal += 1;
            cursor = Some((
                tab.project_id.clone(),
                tab.position,
                tab.created_at,
                tab.id.clone(),
            ));
            owners.insert(tab.id, tab.project_id);
        }
    }
    Ok(owners)
}

fn tab_page(
    transaction: &Transaction<'_>,
    cursor: Option<&(String, i64, i64, String)>,
) -> AppResult<Vec<LegacyTab>> {
    let (project, position, created, id) = cursor
        .cloned()
        .unwrap_or_else(|| (String::new(), i64::MIN, i64::MIN, String::new()));
    let mut statement = transaction.prepare(
        "SELECT id, project_id, title, position, created_at, updated_at
         FROM workbench_tabs
         WHERE kind='chat'
           AND (project_id, position, created_at, id) > (?1, ?2, ?3, ?4)
         ORDER BY project_id, position, created_at, id LIMIT ?5",
    )?;
    let mut rows = statement.query(params![
        project,
        position,
        created,
        id,
        ROWS_PER_PAGE as i64 + 1
    ])?;
    let mut page = Vec::with_capacity(ROWS_PER_PAGE);
    let mut bytes = 0_usize;
    while let Some(row) = rows.next()? {
        if page.len() == ROWS_PER_PAGE {
            break;
        }
        let tab = LegacyTab {
            id: row.get(0)?,
            project_id: row.get(1)?,
            title: row.get(2)?,
            position: row.get(3)?,
            created_at: row.get(4)?,
            updated_at: row.get(5)?,
        };
        let row_bytes = tab.id.len() + tab.project_id.len() + tab.title.len();
        if !page.is_empty() && bytes.saturating_add(row_bytes) > BYTES_PER_PAGE {
            break;
        }
        bytes = bytes.saturating_add(row_bytes);
        page.push(tab);
    }
    Ok(page)
}

fn convert_sessions(
    transaction: &Transaction<'_>,
    tab_projects: &HashMap<String, String>,
) -> AppResult<()> {
    let mut ordinals = HashMap::<String, i64>::new();
    let mut cursor: Option<(String, i64, String)> = None;
    loop {
        let page = session_page(transaction, cursor.as_ref())?;
        if page.is_empty() {
            break;
        }
        for session in page {
            if tab_projects.get(&session.chat_tab_id) != Some(&session.project_id) {
                return invalid("legacy session does not belong to its project Chat tab");
            }
            validate_session(&session)?;
            let ordinal = ordinals.entry(session.chat_tab_id.clone()).or_default();
            insert_session(transaction, &session, *ordinal)?;
            convert_session_events(transaction, &session)?;
            *ordinal += 1;
            cursor = Some((
                session.chat_tab_id.clone(),
                session.created_at,
                session.id.clone(),
            ));
        }
    }
    Ok(())
}

fn session_page(
    transaction: &Transaction<'_>,
    cursor: Option<&(String, i64, String)>,
) -> AppResult<Vec<LegacySession>> {
    let (chat, created, id) = cursor
        .cloned()
        .unwrap_or_else(|| (String::new(), i64::MIN, String::new()));
    let mut statement = transaction.prepare(
        "SELECT id, project_id, chat_tab_id, harness_id, harness_version,
                model_id, external_session_id, last_committed_sequence,
                created_at, updated_at
         FROM legacy_agent_sessions_v4
         WHERE (chat_tab_id, created_at, id) > (?1, ?2, ?3)
         ORDER BY chat_tab_id, created_at, id LIMIT ?4",
    )?;
    let mut rows = statement.query(params![chat, created, id, ROWS_PER_PAGE as i64 + 1])?;
    let mut page = Vec::with_capacity(ROWS_PER_PAGE);
    let mut bytes = 0_usize;
    while let Some(row) = rows.next()? {
        if page.len() == ROWS_PER_PAGE {
            break;
        }
        let session = LegacySession {
            id: row.get(0)?,
            project_id: row.get(1)?,
            chat_tab_id: row.get(2)?,
            binding_id: row.get(3)?,
            harness_version: row.get(4)?,
            model_id: row.get(5)?,
            external_session_id: row.get(6)?,
            last_sequence: row.get(7)?,
            created_at: row.get(8)?,
            updated_at: row.get(9)?,
        };
        let row_bytes = session_metadata_bytes(&session);
        if !page.is_empty() && bytes.saturating_add(row_bytes) > BYTES_PER_PAGE {
            break;
        }
        bytes = bytes.saturating_add(row_bytes);
        page.push(session);
    }
    Ok(page)
}

fn insert_session(
    transaction: &Transaction<'_>,
    session: &LegacySession,
    ordinal: i64,
) -> AppResult<()> {
    let binding_digest = legacy_binding_digest(&session.binding_id, &session.harness_version);
    transaction.execute(
        "INSERT INTO agent_sessions (
           id, project_id, chat_id, ordinal, binding_id, binding_digest,
           harness_version, model_id, external_session_id, capabilities_json,
           state, residency, legacy, revision, projection_revision,
           created_at, updated_at, terminal_at
         ) VALUES (
           ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10,
           'closed', 'cold', 1, 0, 0, ?11, ?12, ?12
         )",
        params![
            session.id,
            session.project_id,
            session.chat_tab_id,
            ordinal,
            session.binding_id,
            binding_digest,
            session.harness_version,
            session.model_id,
            session.external_session_id,
            LEGACY_CAPABILITIES,
            session.created_at,
            session.updated_at
        ],
    )?;
    transaction.execute(
        "INSERT INTO session_runtime_epochs (
           project_id, chat_id, session_id, runtime_epoch, state,
           artifact_digest, started_at, terminal_at, exit_kind
         ) VALUES (?1, ?2, ?3, 0, 'legacy', NULL, ?4, ?5, 'legacyMigration')",
        params![
            session.project_id,
            session.chat_tab_id,
            session.id,
            session.created_at,
            session.updated_at
        ],
    )?;
    Ok(())
}

fn rebuild_and_verify_ledgers(transaction: &Transaction<'_>) -> AppResult<()> {
    transaction.execute_batch(include_str!("ledger.sql"))?;
    let mismatch: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM acp_session_usage u
           WHERE u.turn_count <> (SELECT COUNT(*) FROM turns t WHERE t.session_id=u.session_id)
              OR u.item_count <> (SELECT COUNT(*) FROM items i WHERE i.session_id=u.session_id)
              OR u.fact_count <> (SELECT COUNT(*) FROM session_facts f WHERE f.session_id=u.session_id)
              OR u.item_bytes <> COALESCE((SELECT SUM(payload_bytes) FROM items i WHERE i.session_id=u.session_id),0)
              OR u.fact_bytes <> COALESCE((SELECT SUM(payload_bytes) FROM session_facts f WHERE f.session_id=u.session_id),0)
           LIMIT 1
         )",
        [],
        |row| row.get(0),
    )?;
    if mismatch {
        return invalid("ACP migration ledger verification failed");
    }
    Ok(())
}

fn validate_session(session: &LegacySession) -> AppResult<()> {
    validate_text("legacy session id", &session.id, 1, 512)?;
    validate_text("legacy binding id", &session.binding_id, 1, 512)?;
    for (label, value) in [
        ("legacy harness version", session.harness_version.as_deref()),
        ("legacy model id", session.model_id.as_deref()),
        (
            "legacy external session id",
            session.external_session_id.as_deref(),
        ),
    ] {
        if let Some(value) = value {
            validate_text(label, value, 1, 512)?;
        }
    }
    Ok(())
}

fn validate_text(label: &str, value: &str, min: usize, max: usize) -> AppResult<()> {
    if !(min..=max).contains(&value.len()) {
        return invalid(&format!(
            "{label} violates its {min}..={max} UTF-8 byte contract"
        ));
    }
    Ok(())
}

fn session_metadata_bytes(session: &LegacySession) -> usize {
    session.id.len()
        + session.project_id.len()
        + session.chat_tab_id.len()
        + session.binding_id.len()
        + session.harness_version.as_ref().map_or(0, String::len)
        + session.model_id.as_ref().map_or(0, String::len)
        + session.external_session_id.as_ref().map_or(0, String::len)
}

fn invalid<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}
