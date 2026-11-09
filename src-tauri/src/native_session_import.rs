//! "Open a Codex CLI session": a native session started outside Sage in
//! this Project becomes a new Chat that resumes it. The harness lists and
//! reads the sessions; Rust owns the Chat, its resumable head session and
//! the history read at import.

use tauri::{Manager, State};

use crate::{
    acp::{
        bindings::codex::NativeSessionsPage,
        contract::{SageChatId, SageProjectId},
        intent::ExternalId,
        scheduler::RuntimeOwner,
        service::ImportedHistory,
        store::{
            StoreError,
            native_import::{imported_history, install_import},
        },
    },
    chat_fork::{CHAT_TITLE_PREFIX, committed_transition, insert_layout_chat, stale},
    chat_lifecycle::MAX_CHAT_TITLE_BYTES,
    domain::{ProjectLayoutRequest, ProjectTransitionResult},
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    state::AppState,
};

const MAX_CURSOR_BYTES: usize = 1024;
const IMPORT_UNFINISHED: &str = "The session did not open; try again";

/// `Chat: <title>`, with whitespace collapsed and shortened at a character
/// boundary to the Chat title byte limit.
fn import_title(title: &str) -> String {
    let mut chat = CHAT_TITLE_PREFIX.to_owned();
    for word in title.split_whitespace() {
        let separator = usize::from(chat.len() > CHAT_TITLE_PREFIX.len());
        if chat.len() + separator + word.len() > MAX_CHAT_TITLE_BYTES {
            break;
        }
        if separator == 1 {
            chat.push(' ');
        }
        chat.push_str(word);
    }
    if chat.len() == CHAT_TITLE_PREFIX.len() {
        chat.push_str("Imported session");
    }
    chat
}

impl Database {
    /// Creates the Chat for an imported session in one transaction: Chat row
    /// and tab, title, resumable head session and its history.
    pub(crate) fn create_import_chat(
        &self,
        current_layout: &ProjectLayoutRequest,
        lister: &RuntimeOwner,
        thread: &ExternalId,
        title: &str,
        history_json: &str,
    ) -> AppResult<String> {
        if current_layout.project_id != lister.project_id.as_str() {
            return Err(AppError::InvalidRequest(
                "The session belongs to another Project".into(),
            ));
        }
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let chat_id = insert_layout_chat(
            &transaction,
            current_layout,
            "Open a session in its own active Project",
        )?;
        let title = import_title(title);
        transaction.execute(
            "UPDATE chats SET title=?3,title_is_default=0 WHERE project_id=?1 AND id=?2",
            rusqlite::params![current_layout.project_id, chat_id, title],
        )?;
        transaction.execute(
            "UPDATE workbench_tabs SET title=?3 WHERE project_id=?1 AND id=?2",
            rusqlite::params![current_layout.project_id, chat_id, title],
        )?;
        let chat = SageChatId::parse(chat_id.clone())
            .map_err(|_| AppError::InvalidRequest("imported Chat id is invalid".into()))?;
        install_import(
            &transaction,
            lister,
            &chat,
            thread,
            history_json,
            now_millis(),
        )
        .map_err(|error| {
            AppError::InvalidRequest(match error {
                StoreError::Conflict {
                    resource: "importedSession",
                } => "This session is already open in a Sage chat".into(),
                error => error.to_string(),
            })
        })?;
        transaction.commit()?;
        Ok(chat_id)
    }
}

/// One page of the active Project's importable sessions, most recently
/// active first. A Codex chat of the Project must be running to list them.
#[tauri::command]
pub async fn list_native_sessions(
    app: tauri::AppHandle,
    project_id: String,
    cursor: Option<String>,
) -> Result<NativeSessionsPage, String> {
    let project = SageProjectId::parse(project_id).map_err(|error| error.to_string())?;
    if cursor
        .as_ref()
        .is_some_and(|cursor| cursor.len() > MAX_CURSOR_BYTES)
    {
        return Err("The session list cursor is invalid".into());
    }
    tauri::async_runtime::spawn_blocking(move || {
        app.state::<AppState>()
            .acp
            .list_importable_sessions(&project, cursor)
    })
    .await
    .map_err(|_| IMPORT_UNFINISHED.to_owned())?
    .map_err(|error| error.to_string())
}

/// Opens a listed session as a new Chat. The history is read on a blocking
/// thread outside the Project transition lock; the lock is retaken and the
/// epoch re-validated before the Chat is committed.
#[tauri::command]
pub async fn import_native_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    thread_id: String,
    title: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let project = SageProjectId::parse(current_layout.project_id.clone())
        .map_err(|error| error.to_string())?;
    let thread =
        ExternalId::new("externalSessionId", thread_id).map_err(|error| error.to_string())?;
    stale(
        *state.project_transitions.lock().await,
        expected_transition_epoch,
    )?;
    let handle = app.clone();
    let native = thread.clone();
    let (lister, history) = tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<AppState>()
            .acp
            .read_import_history(&project, native.as_str())
    })
    .await
    .map_err(|_| IMPORT_UNFINISHED.to_owned())?
    .map_err(|error| error.to_string())?;
    let mut epoch = state.project_transitions.lock().await;
    stale(*epoch, expected_transition_epoch)?;
    let database = state.database.clone();
    let layout = current_layout.clone();
    tauri::async_runtime::spawn_blocking(move || {
        database.create_import_chat(&layout, &lister, &thread, &title, &history)
    })
    .await
    .map_err(|_| IMPORT_UNFINISHED.to_owned())?
    .map_err(|error| error.to_string())?;
    Ok(committed_transition(&state, &mut epoch).await)
}

/// The history an imported Chat continues from, oldest Turn first.
#[tauri::command]
pub async fn read_imported_history(
    state: State<'_, AppState>,
    project_id: String,
    chat_id: String,
) -> Result<Option<ImportedHistory>, String> {
    let project = SageProjectId::parse(project_id).map_err(|error| error.to_string())?;
    let chat = SageChatId::parse(chat_id).map_err(|error| error.to_string())?;
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        imported_history(&database.connection(), &project, &chat)
            .map_err(|error| error.to_string())?
            .map(|json| serde_json::from_str(&json).map_err(|error| error.to_string()))
            .transpose()
    })
    .await
    .map_err(|_| "The imported history could not be read".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn import_titles_collapse_whitespace_and_stay_within_the_title_limit() {
        assert_eq!(import_title("  Fix\n the   build "), "Chat: Fix the build");
        assert_eq!(import_title(" \t"), "Chat: Imported session");
        let long = import_title(&"word ".repeat(MAX_CHAT_TITLE_BYTES));
        assert!(long.len() <= MAX_CHAT_TITLE_BYTES && long.ends_with("word"));
    }
}
