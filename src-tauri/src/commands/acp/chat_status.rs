//! Bounded rail metadata; never loads transcript pages or item payloads.
use rusqlite::{Connection, OptionalExtension, params};

use super::*;
use crate::acp::{
    contract::{SafeText, TurnState},
    store::StoreError,
};

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
        "SELECT c.title,t.state,t.updated_at,t.created_at,t.terminal_at FROM chats c LEFT JOIN turns t ON t.id=(
         SELECT latest.id FROM turns latest
         WHERE latest.session_id=(SELECT s.id FROM agent_sessions s
           WHERE s.project_id=c.project_id AND s.chat_id=c.id
             AND EXISTS(SELECT 1 FROM turns present WHERE present.session_id=s.id AND present.id<>?3)
           ORDER BY s.ordinal DESC,s.id DESC LIMIT 1)
         AND latest.id<>?3 ORDER BY latest.ordinal DESC,latest.id DESC LIMIT 1)
         WHERE c.project_id=?1 AND c.id=?2",
        params![project_id.as_str(),chat_id.as_str(),excluded_turn.unwrap_or("")],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,Option<i64>>(2)?,row.get::<_,Option<i64>>(3)?,row.get::<_,Option<i64>>(4)?)),
    ).optional()?.ok_or(StoreError::Conflict { resource: "chat" })?;
    let latest_turn = row
        .1
        .map(|state| {
            Ok::<_, StoreError>(TurnStatus {
                state: crate::acp::store::parse_turn_state(&state)?,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_latest_session_and_turn_without_any_transcript_tables() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE chats(project_id TEXT,id TEXT,title TEXT);
          CREATE TABLE agent_sessions(id TEXT,ordinal INTEGER,project_id TEXT,chat_id TEXT);
          CREATE TABLE turns(id TEXT,project_id TEXT,chat_id TEXT,session_id TEXT,ordinal INTEGER,state TEXT,updated_at INTEGER,created_at INTEGER,terminal_at INTEGER);
          INSERT INTO chats VALUES('p','c','Chat');
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
                    resource: crate::acp::intent::AcpConflictResource::ProjectGeneration,
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
