use tauri::{Manager, State};

use crate::{
    acp::{
        contract::{SageChatId, SageProjectId},
        intent::ExternalId,
        scheduler::RuntimeOwner,
        service::{ImportLister, ImportedHistory, ListedSessions},
        store::{
            StoreError,
            native_import::{imported_session, install_import},
        },
    },
    chat_fork::{committed_transition, insert_layout_chat, stale},
    chat_lifecycle::{CHAT_TITLE_PREFIX, MAX_CHAT_TITLE_BYTES, canonical_chat_title},
    domain::{ProjectLayoutRequest, ProjectTransitionResult},
    persistence::Database,
    state::AppState,
};

use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};

const MAX_CURSOR_BYTES: usize = 1024;
const IMPORT_UNFINISHED: &str = "The session did not open; try again";
const UNTITLED_IMPORT: &str = "Imported session";

fn import_title(title: &str) -> String {
    let mut chat = String::new();
    for word in title
        .split(|character: char| character.is_whitespace() || character.is_control())
        .filter(|word| !word.is_empty())
    {
        let separator = usize::from(!chat.is_empty());
        if chat.len() + separator + word.len() > MAX_CHAT_TITLE_BYTES {
            break;
        }
        if separator == 1 {
            chat.push(' ');
        }
        chat.push_str(word);
    }
    canonical_chat_title(&chat).unwrap_or_else(|_| format!("{CHAT_TITLE_PREFIX}{UNTITLED_IMPORT}"))
}

impl Database {
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
        // The title is the session's own name or preview as the harness
        // listed it, so a name the harness reports later may replace it.
        transaction.execute(
            "UPDATE chats SET title=?3,title_source=?4 WHERE project_id=?1 AND id=?2",
            rusqlite::params![
                current_layout.project_id,
                chat_id,
                title,
                crate::chat_lifecycle::ChatTitleSource::Harness.as_str()
            ],
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

#[tauri::command]
pub async fn list_native_sessions(
    app: tauri::AppHandle,
    project_id: String,
    cursor: Option<String>,
    lister: Option<ImportLister>,
) -> Result<ListedSessions, String> {
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
            .list_importable_sessions(&project, cursor, lister.as_ref())
    })
    .await
    .map_err(|_| IMPORT_UNFINISHED.to_owned())?
    .map_err(|error| error.to_string())
}

const STALE_IMPORT: &str = "opening the session";

/// Opens a session `lister` listed as a new Chat. The history is read on a
/// blocking thread outside the Project transition lock; the lock is retaken
/// and the epoch re-validated before the Chat is committed.
#[tauri::command]
pub async fn import_native_session(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    lister: ImportLister,
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
        STALE_IMPORT,
    )?;
    let handle = app.clone();
    let native = thread.clone();
    let (lister, history) = tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<AppState>()
            .acp
            .read_import_history(&project, &lister, native.as_str())
    })
    .await
    .map_err(|_| IMPORT_UNFINISHED.to_owned())?
    .map_err(|error| error.to_string())?;
    let mut epoch = state.project_transitions.lock().await;
    stale(*epoch, expected_transition_epoch, STALE_IMPORT)?;
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

/// Oldest Turn first.
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ImportedHistoryView {
    session_id: String,
    runtime_epoch: String,
    #[serde(flatten)]
    history: ImportedHistory,
}

#[tauri::command]
pub async fn read_imported_history(
    state: State<'_, AppState>,
    project_id: String,
    chat_id: String,
) -> Result<Option<ImportedHistoryView>, String> {
    let project = SageProjectId::parse(project_id).map_err(|error| error.to_string())?;
    let chat = SageChatId::parse(chat_id).map_err(|error| error.to_string())?;
    let database = state.database.clone();
    tauri::async_runtime::spawn_blocking(move || {
        let Some(imported) = imported_session(&database.connection(), &project, &chat, true)
            .map_err(|error| error.to_string())?
        else {
            return Ok(None);
        };
        let json = imported.history_json.unwrap_or_default();
        Ok(Some(ImportedHistoryView {
            session_id: imported.session_id,
            runtime_epoch: imported.runtime_epoch.to_string(),
            history: serde_json::from_str(&json).map_err(|error| error.to_string())?,
        }))
    })
    .await
    .map_err(|_| "The imported history could not be read".to_owned())?
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chat_lifecycle::chat_display_title;

    #[test]
    fn import_titles_collapse_whitespace_and_stay_within_the_title_limit() {
        let shown = |title: &str| chat_display_title(&import_title(title)).to_owned();
        assert_eq!(import_title("  Fix\n the   build "), "Chat: Fix the build");
        assert_eq!(shown(" \t"), UNTITLED_IMPORT);
        assert_eq!(shown("\u{feff}"), UNTITLED_IMPORT);
        assert_eq!(shown("Fix\u{7}the build"), "Fix the build");
        let long = shown(&"word ".repeat(MAX_CHAT_TITLE_BYTES));
        assert!(long.len() <= MAX_CHAT_TITLE_BYTES && long.ends_with("word"));
    }

    #[test]
    fn an_imported_title_that_starts_like_the_stored_prefix_shows_whole() {
        assert_eq!(import_title("Chat: notes"), "Chat: Chat: notes");
        assert_eq!(
            chat_display_title(&import_title("Chat: notes")),
            "Chat: notes"
        );
    }
}
