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
    chat_listed: bool,
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
        &format!(concat!(
            "SELECT c.title,t.state,t.updated_at,t.created_at,t.terminal_at,{} FROM chats c LEFT JOIN turns t ON t.id=(
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
        ), crate::chat_lifecycle::visibility::listed_chat()),
        params![project_id.as_str(),chat_id.as_str(),excluded_turn.unwrap_or("")],
        |row| Ok((row.get::<_,String>(0)?,row.get::<_,Option<String>>(1)?,row.get::<_,Option<i64>>(2)?,row.get::<_,Option<i64>>(3)?,row.get::<_,Option<i64>>(4)?,row.get::<_,bool>(5)?)),
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
        chat_listed: row.5,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_reads_latest_session_and_turn_without_any_transcript_tables() {
        let db = Connection::open_in_memory().unwrap();
        db.execute_batch("CREATE TABLE chats(project_id TEXT,id TEXT,title TEXT,title_source TEXT,created_at INTEGER);
          CREATE TABLE chat_tabs(project_id TEXT,chat_id TEXT,tab_id TEXT);
          CREATE TABLE chat_lineage(child_project_id TEXT,child_chat_id TEXT,relation TEXT);
          CREATE TABLE workbench_tabs(id TEXT,pinned INTEGER);
          CREATE TABLE chat_queued_prompts(project_id TEXT,chat_id TEXT,prompts_json TEXT);
          CREATE TABLE agent_sessions(id TEXT,ordinal INTEGER,project_id TEXT,chat_id TEXT,
            binding_id TEXT,binding_digest TEXT,external_session_id TEXT,model_id TEXT,state TEXT,
            residency TEXT,legacy INTEGER,revision INTEGER,projection_revision INTEGER,
            created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
          CREATE TABLE acp_session_usage(session_id TEXT,turn_count INTEGER,item_count INTEGER,fact_count INTEGER,revision INTEGER);
          CREATE TABLE chat_session_heads(session_id TEXT);
          CREATE TABLE session_runtime_epochs(session_id TEXT,runtime_epoch INTEGER,state TEXT,exit_kind TEXT,started_at INTEGER,terminal_at INTEGER);
          CREATE TABLE items(turn_id TEXT,ordinal INTEGER,origin TEXT,kind TEXT,lifecycle TEXT,revision INTEGER,payload_json TEXT,created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
          CREATE TABLE turns(id TEXT,project_id TEXT,chat_id TEXT,session_id TEXT,ordinal INTEGER,state TEXT,updated_at INTEGER,created_at INTEGER,terminal_at INTEGER,runtime_epoch INTEGER,delivery_state TEXT,client_intent_id TEXT,revision INTEGER);
          CREATE TABLE session_facts(session_id TEXT,turn_id TEXT,kind TEXT,sequence INTEGER,payload_json TEXT,projection_revision INTEGER,created_at INTEGER);
          INSERT INTO chats(project_id,id,title,title_source) VALUES('p','c','Chat','default');
          CREATE INDEX idx_agent_sessions_owner ON agent_sessions(project_id,chat_id,ordinal,id);
          CREATE INDEX idx_turns_history ON turns(session_id,ordinal DESC,id DESC);
          INSERT INTO agent_sessions(id,ordinal,project_id,chat_id) VALUES('old',0,'p','c'),('new',1,'p','c'),('empty',2,'p','c');
          INSERT INTO turns(id,project_id,chat_id,session_id,ordinal,state,updated_at,created_at,terminal_at) VALUES('old-turn','p','c','old',99,'completed',1,1,1),('first','p','c','new',0,'completed',2,2,2),('latest','p','c','new',1,'waitingApproval',3,3,NULL);").unwrap();
        let status = load(
            &db,
            SageProjectId::parse("p").unwrap(),
            SageChatId::parse("c").unwrap(),
            None,
        )
        .unwrap();
        assert_eq!(
            serde_json::to_value(status).unwrap(),
            serde_json::json!({"projectId":"p","chatId":"c","title":"Chat","chatListed":true,"latestTurn":{"state":"waitingApproval","updatedAt":3,"createdAt":3,"terminalAt":null}})
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
            "INSERT INTO session_facts(session_id,turn_id,kind) VALUES('new','latest','turnUndone')",
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
