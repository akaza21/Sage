use crate::chat_lifecycle::store::ChatLifecycleStore;
use crate::side_chat_storage::SideChatStore;
use crate::state::AppState;
use sage_acp_protocol::contract::{SageChatId, SageProjectId};
use sage_db::database::Database;
use sage_domain::{chat::ProjectLayoutRequest, project::ProjectTransitionResult};
use tauri::{Manager, State};

#[tauri::command]
pub async fn open_side_chat(
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    parent_chat_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let project = current_layout.project_id.clone();
    Ok(crate::chat_commands::execute_chat_transition(
        &state,
        expected_transition_epoch,
        "Project changed before opening side chat",
        move |database| {
            database
                .create_chat_with_parent(&project, &current_layout, Some(&parent_chat_id))
                .map(drop)
        },
    )
    .await)
}

fn owned_side_chat(
    state: &AppState,
    epoch: u64,
    expected_transition_epoch: u64,
    project_id: &str,
    chat_id: &str,
) -> Result<(), String> {
    if epoch != expected_transition_epoch || epoch >= 9_007_199_254_740_991 {
        return Err("Project changed before closing side chat".into());
    }
    if !chat_id.starts_with(crate::chat_lifecycle::SIDE_CHAT_ID_PREFIX) {
        return Err("not a temporary Chat".into());
    }
    let active: Option<String> = state
        .database
        .connection()
        .query_row(
            "SELECT active_project_id FROM app_state WHERE singleton=1",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if active.as_deref() != Some(project_id) {
        return Err("Side chat belongs to a different Project".into());
    }
    Ok(())
}

/// Deletes the Chat, then (only once that succeeded) records the opt-out.
fn finish_close(
    database: &sage_db::database::Database,
    settings: &crate::settings_store::SettingsStore,
    project_id: &str,
    chat_id: &str,
    dont_ask_again: bool,
) -> Result<(), String> {
    database
        .remove_side_chat(project_id, chat_id)
        .map_err(|error| error.to_string())?;
    crate::settings_store::forget_removed_chat_access(database, settings);
    if dont_ask_again {
        settings
            .set_confirm_close_side_chat(false)
            .map_err(|error| error.to_string())?;
    }
    Ok(())
}

/// Ending the runtime can wait on the harness for its explicit-close
/// deadline, so it runs on a blocking thread and outside the Project
/// transition lock. That is safe: disposal is idempotent and touches only
/// this temporary Chat's runtime. The lock is retaken and the epoch and
/// owner re-validated before anything durable changes. A runtime started in
/// the gap is caught by the running-epoch check and the close is refused; a
/// prompt still being prepared without a runtime is deleted with the Chat and
/// its send then fails as not found. A concurrent close that already removed
/// the Chat is reported as already closed.
#[tauri::command]
pub async fn close_side_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    project_id: String,
    chat_id: String,
    expected_transition_epoch: u64,
    dont_ask_again: bool,
) -> Result<ProjectTransitionResult, String> {
    {
        let epoch = state.project_transitions.lock().await;
        owned_side_chat(
            &state,
            *epoch,
            expected_transition_epoch,
            &project_id,
            &chat_id,
        )?;
    }
    let project = SageProjectId::parse(project_id.clone()).map_err(|error| error.to_string())?;
    let chat = SageChatId::parse(chat_id.clone()).map_err(|error| error.to_string())?;
    let handle = app.clone();
    tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<AppState>()
            .acp
            .dispose_temporary_runtime(&project, &chat)
    })
    .await
    .map_err(|_| "Side chat shutdown is unsettled; try closing again".to_owned())?
    .map_err(|error| format!("Side chat shutdown is unsettled: {error:?}"))?;
    let mut epoch = state.project_transitions.lock().await;
    let exists: bool = state
        .database
        .connection()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1 AND id=?2)",
            rusqlite::params![project_id, chat_id],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    if !exists {
        return Err("This side chat was already closed".into());
    }
    owned_side_chat(
        &state,
        *epoch,
        expected_transition_epoch,
        &project_id,
        &chat_id,
    )?;
    let running: bool = state.database.connection().query_row("SELECT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE project_id=?1 AND chat_id=?2 AND state IN ('starting','running','terminating'))", rusqlite::params![project_id,chat_id], |row| row.get(0)).map_err(|error| error.to_string())?;
    if running {
        return Err("Side chat shutdown is unsettled; try closing again".into());
    }
    finish_close(
        &state.database,
        &state.settings,
        &project_id,
        &chat_id,
        dont_ask_again,
    )?;
    *epoch += 1;
    crate::preview_lifecycle::on_committed_project_transition_epoch(*epoch);
    let snapshot = crate::resource_commands::build_bootstrap_snapshot(&state, *epoch).await;
    Ok(ProjectTransitionResult {
        committed: true,
        transition_epoch: *epoch,
        snapshot: snapshot.as_ref().ok().cloned(),
        recovery_error: snapshot.err().map(|error| error.to_string()),
    })
}

#[tauri::command]
pub async fn get_confirm_close_side_chat(state: State<'_, AppState>) -> Result<bool, String> {
    get_confirm_close_side_chat_async(&state.settings).await
}

async fn get_confirm_close_side_chat_async(
    settings: &crate::settings_store::SettingsStore,
) -> Result<bool, String> {
    let settings = settings.clone();
    tauri::async_runtime::spawn_blocking(move || {
        settings
            .confirm_close_side_chat()
            .map_err(|error| error.to_string())
    })
    .await
    .map_err(|error| error.to_string())?
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SideChatSummary {
    id: String,
    project_id: String,
    parent_chat_id: String,
    running: bool,
}

#[tauri::command]
pub async fn list_side_chats(state: State<'_, AppState>) -> Result<Vec<SideChatSummary>, String> {
    list_side_chats_async(&state.database).await
}

async fn list_side_chats_async(database: &Database) -> Result<Vec<SideChatSummary>, String> {
    let database = database.clone();
    tauri::async_runtime::spawn_blocking(move || list_side_chats_inner(&database))
        .await
        .map_err(|error| error.to_string())?
}

fn list_side_chats_inner(database: &Database) -> Result<Vec<SideChatSummary>, String> {
    let connection = database.connection();
    let mut statement = connection.prepare("SELECT w.id,w.project_id,json_extract(w.metadata_json,'$.parentChatId'),EXISTS(SELECT 1 FROM turns t WHERE t.project_id=w.project_id AND t.chat_id=w.id AND t.state NOT IN ('completed','cancelled','failed','interrupted','deliveryUnknown')) FROM workbench_tabs w WHERE w.kind='chat' AND json_extract(w.metadata_json,'$.ephemeral')=1 ORDER BY w.id LIMIT 9").map_err(|error| error.to_string())?;
    let rows = statement
        .query_map([], |row| {
            Ok(SideChatSummary {
                id: row.get(0)?,
                project_id: row.get(1)?,
                parent_chat_id: row.get(2)?,
                running: row.get(3)?,
            })
        })
        .map_err(|error| error.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| error.to_string())?;
    if rows.len() > 8 {
        return Err("temporary Chat registry exceeds eight".into());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_lifecycle::store::ChatLifecycleStore;
    use crate::project_registry::create::ProjectCreateStore;
    use crate::workbench_persistence::store::WorkbenchStore;

    #[tokio::test]
    async fn side_chat_lookup_reports_parent_and_running_and_refuses_overflow() {
        let root = std::env::temp_dir().join(format!("sage-side-list-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let project = database.create_project_from_root(&root).unwrap().id;
        let parent = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        // Seed registry states directly: normal creation permits only one side chat per Project.
        let add_side = || {
            let mut connection = database.connection();
            let transaction = connection.transaction().unwrap();
            let id =
                crate::chat_lifecycle::insert_new_chat(&transaction, &project, true, Some(&parent))
                    .unwrap();
            transaction.commit().unwrap();
            id
        };
        let running = add_side();
        let idle = add_side();
        {
            let connection = database.connection();
            connection.execute(
                "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
                 capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at)
                 VALUES('side-session',?1,?2,0,'codex-app-server',?3,'{}','active','hot',0,0,0,1,1)",
                rusqlite::params![project, running, "a".repeat(64)],
            ).unwrap();
            connection.execute(
                "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
                 VALUES(?1,?2,'side-session',1,'running',1)",
                rusqlite::params![project, running],
            ).unwrap();
            connection.execute(
                "INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                 state,delivery_state,revision,created_at,updated_at)
                 VALUES('side-turn',?1,?2,'side-session',1,0,'side-intent','running','correlated',0,1,1)",
                rusqlite::params![project, running],
            ).unwrap();
        }
        let rows = list_side_chats_async(&database).await.unwrap();
        assert_eq!(rows.len(), 2);
        for (id, expected_running) in [(&running, true), (&idle, false)] {
            let row = rows.iter().find(|row| &row.id == id).unwrap();
            assert_eq!(
                serde_json::to_value(row).unwrap(),
                serde_json::json!({
                    "id": id, "projectId": project, "parentChatId": parent, "running": expected_running,
                })
            );
        }
        for _ in 2..8 {
            add_side();
        }
        assert_eq!(list_side_chats_async(&database).await.unwrap().len(), 8);
        add_side();
        assert_eq!(
            list_side_chats_async(&database).await.err().unwrap(),
            "temporary Chat registry exceeds eight"
        );
        drop(database);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn confirm_close_lookup_reads_the_persisted_preference() {
        let root = std::env::temp_dir().join(format!("sage-side-confirm-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let settings =
            crate::settings_store::SettingsStore::open(&root.join("settings.db")).unwrap();
        assert!(get_confirm_close_side_chat_async(&settings).await.unwrap());
        settings.set_confirm_close_side_chat(false).unwrap();
        assert!(!get_confirm_close_side_chat_async(&settings).await.unwrap());
        drop(settings);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn the_opt_out_is_saved_only_after_the_side_chat_is_gone() {
        let root = std::env::temp_dir().join(format!("sage-side-close-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let settings =
            crate::settings_store::SettingsStore::open(&root.join("settings.db")).unwrap();
        database.create_project_from_root(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.as_ref().unwrap().id.clone();
        let parent = snapshot.active_tab_id.clone().unwrap();
        let layout = ProjectLayoutRequest {
            project_id: project.clone(),
            tabs: snapshot.tabs,
            active_tab_id: Some(parent.clone()),
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        };
        let side = database
            .create_chat_with_parent(&project, &layout, Some(&parent))
            .unwrap();
        let restore: String = database
            .connection()
            .query_row(
                "SELECT sql FROM sqlite_schema WHERE name='deny_chat_delete'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        database
            .connection()
            .execute_batch("DROP TRIGGER deny_chat_delete")
            .unwrap();
        assert!(finish_close(&database, &settings, &project, &side, true).is_err());
        assert!(settings.confirm_close_side_chat().unwrap());
        database.connection().execute_batch(&restore).unwrap();
        finish_close(&database, &settings, &project, &side, true).unwrap();
        assert!(!settings.confirm_close_side_chat().unwrap());
        drop((database, settings));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn closing_a_side_chat_drops_its_sessions_access_grants() {
        use sage_acp_protocol::access_mode::SessionAccessMode::FullAccess;
        let root = std::env::temp_dir().join(format!("sage-side-grant-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let settings = crate::settings_store::SettingsStore::in_memory().unwrap();
        let project = database.create_project_from_root(&root).unwrap().id;
        let parent = database
            .bootstrap_snapshot()
            .unwrap()
            .active_tab_id
            .unwrap();
        let side = {
            let mut connection = database.connection();
            let transaction = connection.transaction().unwrap();
            let id =
                crate::chat_lifecycle::insert_new_chat(&transaction, &project, true, Some(&parent))
                    .unwrap();
            transaction.commit().unwrap();
            id
        };
        for (session, chat) in [("side-session", &side), ("parent-session", &parent)] {
            database.connection().execute(
                "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,
                 capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at)
                 VALUES(?1,?2,?3,0,'claude-agent-acp',?4,'{}','active','hot',0,0,0,1,1)",
                rusqlite::params![session, project, chat, "a".repeat(64)],
            ).unwrap();
            let token = settings
                .prepare_chat_access(session, FullAccess, 1)
                .unwrap();
            settings
                .grant_chat_access(session, FullAccess, &token, 2)
                .unwrap();
        }
        finish_close(&database, &settings, &project, &side, false).unwrap();
        assert_eq!(settings.chat_access("side-session").unwrap(), None);
        assert_eq!(
            settings.chat_access("parent-session").unwrap(),
            Some(FullAccess)
        );
        drop((database, settings));
        std::fs::remove_dir_all(root).unwrap();
    }
}
