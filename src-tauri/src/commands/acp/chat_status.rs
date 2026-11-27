//! Bounded rail metadata; never loads transcript pages or item payloads.
use rusqlite::{Connection, OptionalExtension, params};

use super::*;
use crate::acp::store::StoreError;
use sage_acp_protocol::{contract::SafeText, lifecycle::TurnState};

#[derive(Debug, Clone, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChatStatusRequest {
    project_id: SageProjectId,
    project_generation: ProjectGeneration,
    chat_id: SageChatId,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatStatus {
    project_id: SageProjectId,
    chat_id: SageChatId,
    title: SafeText<8192>,
    latest_turn: Option<TurnStatus>,
}

#[derive(Debug, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct TurnStatus {
    state: TurnState,
    updated_at: i64,
    created_at: i64,
    terminal_at: Option<i64>,
    /// The turn failed because the harness needs a sign-in.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    sign_in: bool,
    /// The turn has settled while its session still runs background work.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    background: bool,
}

#[tauri::command]
pub async fn get_acp_chat_status(
    state: State<'_, AppState>,
    request: ChatStatusRequest,
) -> Result<ChatStatus, AcpIpcError> {
    get_acp_chat_status_inner(&state, request).await
}

async fn get_acp_chat_status_inner(
    state: &AppState,
    request: ChatStatusRequest,
) -> Result<ChatStatus, AcpIpcError> {
    let _read = state
        .acp
        .reserve_page_read(&request.project_id, &request.chat_id)
        .map_err(page_admission_error)?;
    let _transition =
        lock_acp_context(state, &request.project_id, request.project_generation).await?;
    let connection = state.database.connection();
    let welcome =
        crate::session_events::legacy_welcome::turn_id(&connection, request.chat_id.as_str())
            .map_err(|error| store_error(StoreError::Database(error)))?;
    load(
        &connection,
        request.project_id,
        request.chat_id,
        welcome.as_deref(),
    )
    .map_err(store_error)
}

fn load(
    connection: &Connection,
    project_id: SageProjectId,
    chat_id: SageChatId,
    excluded_turn: Option<&str>,
) -> Result<ChatStatus, StoreError> {
    let row = connection.query_row(
        concat!(
            "SELECT c.title,t.state,t.updated_at,t.created_at,t.terminal_at,t.id,t.session_id FROM chats c LEFT JOIN turns t ON t.id=(
             SELECT latest.id FROM turns latest
             WHERE latest.session_id=(SELECT s.id FROM agent_sessions s
               WHERE s.project_id=c.project_id AND s.chat_id=c.id
                 AND EXISTS(SELECT 1 FROM turns present WHERE present.session_id=s.id AND present.id<>?3 AND ",
            crate::acp::store::turn_undo::visible_turn!("present.session_id", "present.id"),
            ")
               ORDER BY s.ordinal DESC,s.id DESC LIMIT 1)
             AND latest.id<>?3 AND ",
            crate::acp::store::turn_undo::visible_turn!("latest.session_id", "latest.id"),
            " ORDER BY latest.ordinal DESC,latest.id DESC LIMIT 1)
             WHERE c.project_id=?1 AND c.id=?2"
        ),
        params![project_id.as_str(),chat_id.as_str(),excluded_turn.unwrap_or("")],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,Option<i64>>(2)?,row.get::<_,Option<i64>>(3)?,row.get::<_,Option<i64>>(4)?,row.get::<_,Option<String>>(5)?,row.get::<_,Option<String>>(6)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource: "chat" })?;
    let latest_turn = row
        .1
        .map(|state| {
            let state = crate::acp::store::parse_turn_state(&state)?;
            let turn = row.5.as_deref().unwrap_or_default();
            let session = row.6.as_deref().unwrap_or_default();
            Ok::<_, StoreError>(TurnStatus {
                sign_in: state == TurnState::Failed && signed_out(connection, turn, session)?,
                background: row.4.is_some()
                    && background_work(connection, &project_id, &chat_id, session)?,
                state,
                updated_at: row.2.ok_or(StoreError::Corrupt("turn timestamp"))?,
                created_at: row.3.ok_or(StoreError::Corrupt("turn timestamp"))?,
                terminal_at: row.4,
            })
        })
        .transpose()?;
    Ok(ChatStatus {
        project_id,
        chat_id,
        title: SafeText::new("chatTitle", row.0).map_err(|_| StoreError::Corrupt("chat title"))?,
        latest_turn,
    })
}

fn signed_out(connection: &Connection, turn: &str, session: &str) -> Result<bool, StoreError> {
    Ok(connection
        .query_row(
            "SELECT 1 FROM acp_turn_terminal_reasons
             WHERE session_id=?1 AND turn_id=?2 AND kind='authRequired' LIMIT 1",
            params![session, turn],
            |_| Ok(()),
        )
        .optional()?
        .is_some())
}

/// The rows Resources lists as background processes: background tasks, and
/// running subagents that can still be stopped, in the live runtime.
fn background_work(
    connection: &Connection,
    project: &SageProjectId,
    chat: &SageChatId,
    session: &str,
) -> Result<bool, StoreError> {
    let epoch: Option<i64> = connection.query_row(
        "SELECT MAX(runtime_epoch) FROM session_runtime_epochs
         WHERE session_id=?1 AND state IN ('starting','running','terminating')",
        params![session],
        |row| row.get(0),
    )?;
    let Some(epoch) = epoch.and_then(|epoch| u64::try_from(epoch).ok()) else {
        return Ok(false);
    };
    // Display decoration: a surface that cannot be read means no background work.
    let view = crate::acp::store::session_surface::view(
        connection,
        project.as_str(),
        chat.as_str(),
        session,
        Some(epoch),
    )
    .unwrap_or(None);
    Ok(view.is_some_and(|view| {
        view.activities.iter().any(|activity| {
            !activity.state.is_terminal()
                && (activity.kind.as_str() == "backgroundTask"
                    || (activity.kind.as_str() == "subagent" && activity.stoppable))
        })
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn status_db() -> Connection {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE chats(project_id TEXT,id TEXT,title TEXT);
          CREATE TABLE agent_sessions(id TEXT,ordinal INTEGER,project_id TEXT,chat_id TEXT);
          CREATE TABLE turns(id TEXT,project_id TEXT,chat_id TEXT,session_id TEXT,ordinal INTEGER,state TEXT,updated_at INTEGER,created_at INTEGER,terminal_at INTEGER);
          CREATE TABLE session_facts(session_id TEXT,turn_id TEXT,kind TEXT);
          CREATE TABLE acp_turn_terminal_reasons(session_id TEXT,turn_id TEXT,kind TEXT);
          CREATE TABLE session_runtime_epochs(session_id TEXT,runtime_epoch INTEGER,state TEXT);
          CREATE TABLE acp_session_surfaces(session_id TEXT,project_id TEXT,chat_id TEXT,surface_json TEXT,revision INTEGER,updated_at INTEGER);
          INSERT INTO chats VALUES('p','c','Chat');").unwrap();
        db
    }

    fn turn_json(db: &Connection) -> serde_json::Value {
        let status = load(
            db,
            SageProjectId::parse("p").unwrap(),
            SageChatId::parse("c").unwrap(),
            None,
        )
        .unwrap();
        serde_json::to_value(status).unwrap()["latestTurn"].clone()
    }

    #[test]
    fn status_names_a_sign_in_failure_and_background_work_after_the_turn() {
        use sage_acp_protocol::{
            contract::RequiredText,
            intent::{
                ExternalId,
                session_event::{
                    SessionActivity, SessionActivityState, SessionScopedEvent, SessionSurface,
                },
            },
        };
        let db = status_db();
        db.execute_batch(
            "INSERT INTO agent_sessions VALUES('s',0,'p','c');
          INSERT INTO turns VALUES('t','p','c','s',0,'failed',2,1,2);
          INSERT INTO acp_turn_terminal_reasons VALUES('s','t','authRequired');",
        )
        .unwrap();
        assert_eq!(turn_json(&db)["signIn"], true);
        db.execute("UPDATE turns SET state='cancelled'", [])
            .unwrap();
        assert_eq!(turn_json(&db).get("signIn"), None);
        db.execute_batch(
            "UPDATE acp_turn_terminal_reasons SET kind='timeout';
          UPDATE turns SET state='completed';
          INSERT INTO session_runtime_epochs VALUES('s',1,'running');",
        )
        .unwrap();
        assert_eq!(turn_json(&db).get("signIn"), None);
        let surface = |kind: &str, state, stoppable| {
            let mut surface = SessionSurface::default();
            surface.apply(
                1,
                &SessionScopedEvent::Activity(SessionActivity {
                    id: ExternalId::new("activity", "task-1").unwrap(),
                    kind: RequiredText::new("kind", kind).unwrap(),
                    label: RequiredText::new("label", "npm run dev").unwrap(),
                    state,
                    progress: None,
                    in_previous_turn: false,
                    stoppable,
                }),
            );
            db.execute(
                "INSERT OR REPLACE INTO acp_session_surfaces VALUES('s','p','c',?1,0,1)",
                [serde_json::to_string(&surface).unwrap()],
            )
            .unwrap();
            db.execute("DELETE FROM acp_session_surfaces WHERE rowid<>(SELECT MAX(rowid) FROM acp_session_surfaces)", [])
                .unwrap();
            turn_json(&db).get("background").cloned()
        };
        let running = SessionActivityState::Running;
        assert_eq!(surface("backgroundTask", running, false), Some(true.into()));
        assert_eq!(surface("subagent", running, true), Some(true.into()));
        // A foreground subagent or a finished task is not background work.
        assert_eq!(surface("subagent", running, false), None);
        assert_eq!(
            surface("backgroundTask", SessionActivityState::Completed, false),
            None
        );
        // A turn that has not settled reports its own state instead.
        surface("backgroundTask", running, false);
        db.execute("UPDATE turns SET terminal_at=NULL", []).unwrap();
        assert_eq!(turn_json(&db).get("background"), None);
        db.execute("UPDATE turns SET terminal_at=2", []).unwrap();
        // Nor is a task whose runtime has ended.
        db.execute("UPDATE session_runtime_epochs SET state='exited'", [])
            .unwrap();
        assert_eq!(turn_json(&db).get("background"), None);
    }

    #[test]
    fn status_reads_latest_session_and_turn_without_any_transcript_tables() {
        let db = status_db();
        db.execute_batch("
          CREATE INDEX idx_agent_sessions_owner ON agent_sessions(project_id,chat_id,ordinal,id);
          CREATE INDEX idx_turns_history ON turns(session_id,ordinal DESC,id DESC);
          INSERT INTO agent_sessions VALUES('old',0,'p','c'),('new',1,'p','c'),('empty',2,'p','c');
          INSERT INTO turns VALUES('old-turn','p','c','old',99,'completed',1,1,1),('first','p','c','new',0,'completed',2,2,2),('latest','p','c','new',1,'waitingApproval',3,3,NULL);").unwrap();
        let status = load(
            &db,
            SageProjectId::parse("p").unwrap(),
            SageChatId::parse("c").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"projectId":"p","chatId":"c","title":"Chat","latestTurn":{"state":"waitingApproval","updatedAt":3,"createdAt":3,"terminalAt":null}})
        );
        assert!(
            load(
                &db,
                SageProjectId::parse("foreign").unwrap(),
                SageChatId::parse("c").unwrap(),
                None
            )
            .is_err()
        );
        db.execute(
            "INSERT INTO session_facts VALUES('new','latest','turnUndone')",
            [],
        )
        .unwrap();
        let status = load(
            &db,
            SageProjectId::parse("p").unwrap(),
            SageChatId::parse("c").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(status.latest_turn.map(|turn| turn.created_at), Some(2));
        db.execute("DELETE FROM turns", []).unwrap();
        assert!(
            load(
                &db,
                SageProjectId::parse("p").unwrap(),
                SageChatId::parse("c").unwrap(),
                None
            )
            .unwrap()
            .latest_turn
            .is_none()
        );
    }

    #[test]
    fn status_command_rejects_stale_generation_and_omits_proven_legacy_welcome() {
        tauri::async_runtime::block_on(async {
            let (state, project, chat, root) = crate::commands::tests::state_with_project();
            {
                let mut connection = state.database.write_connection().unwrap();
                let transaction = connection.transaction().unwrap();
                let created: i64 = transaction
                    .query_row("SELECT created_at FROM chats WHERE id=?1", [&chat], |row| {
                        row.get(0)
                    })
                    .unwrap();
                crate::session_events::seed_legacy_welcome(
                    &transaction,
                    &project,
                    &chat,
                    "legacy-status-session",
                    created,
                    r#"{"role":"assistant","text":"Historical onboarding"}"#,
                    "simulatorTurnCommitted",
                )
                .unwrap();
                transaction.commit().unwrap();
                assert!(
                    crate::session_events::legacy_welcome::turn_id(&connection, &chat)
                        .unwrap()
                        .is_some()
                );
            }
            let request = ChatStatusRequest {
                project_id: SageProjectId::parse(project).unwrap(),
                chat_id: SageChatId::parse(chat).unwrap(),
                project_generation: ProjectGeneration::new(1).unwrap(),
            };
            assert!(matches!(
                get_acp_chat_status_inner(&state, request.clone()).await,
                Err(AcpIpcError::Conflict {
                    resource: crate::acp::ipc::AcpConflictResource::ProjectGeneration,
                    ..
                })
            ));
            assert!(
                get_acp_chat_status_inner(
                    &state,
                    ChatStatusRequest {
                        project_generation: ProjectGeneration::new(0).unwrap(),
                        ..request
                    }
                )
                .await
                .unwrap()
                .latest_turn
                .is_none()
            );
            std::fs::remove_dir_all(root).unwrap();
        });
    }
}
