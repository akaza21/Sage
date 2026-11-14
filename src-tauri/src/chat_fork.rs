//! "Branch in new chat": a native harness fork of a Chat's latest completed
//! Turn becomes a new persistent Chat placed directly above its parent and
//! titled `<parent title> (n)`. The harness owns the forked context; Rust owns
//! the Chat, its resumable head session and the immutable lineage row.

use rusqlite::params;
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use tauri::{Manager, State};

use crate::{
    acp::store::chat_fork::{ForkSource, fork_source, install_fork},
    chat_lifecycle::{
        CHAT_TITLE_PREFIX, MAX_CHAT_TITLE_BYTES, chat_display_title, insert_new_chat,
        validate_chat_identifier,
    },
    domain::{ProjectLayoutRequest, ProjectTransitionResult},
    persistence::Database,
    state::AppState,
    workbench_persistence::{MAX_RESTORED_TABS, persist_layout_in},
};
use sage_acp_protocol::{
    contract::{SageChatId, SageProjectId, SageTurnId},
    intent::ExternalId,
};

/// Fork numbers are searched in a bounded window; a Project holds far fewer
/// Chats than this, so a free number always exists.
const MAX_FORK_NUMBER: u32 = 10_000;

/// `<parent> (n)` with the smallest `n >= 2` no Project Chat already uses.
/// The parent name is shortened at a character boundary so the whole title
/// stays within the Chat title byte limit.
pub(crate) fn fork_title<'a>(
    parent_title: &str,
    taken: impl Iterator<Item = &'a str> + Clone,
) -> String {
    let parent = chat_display_title(parent_title);
    for number in 2..=MAX_FORK_NUMBER {
        let suffix = format!(" ({number})");
        let mut end = parent.len().min(MAX_CHAT_TITLE_BYTES - suffix.len());
        while !parent.is_char_boundary(end) {
            end -= 1;
        }
        let title = format!("{CHAT_TITLE_PREFIX}{}{suffix}", parent[..end].trim_end());
        if !taken.clone().any(|existing| existing == title) {
            return title;
        }
    }
    format!("{CHAT_TITLE_PREFIX}{parent}")
}

/// Why `create_fork_chat` made no fork Chat.
#[derive(Debug)]
pub(crate) enum ForkChatError {
    /// Refused before the commit (stale source, layout or tab limit,
    /// lineage validation): the transaction rolled back, nothing durable
    /// names the native fork, and the caller deletes it.
    Refused(AppError),
    /// `COMMIT` itself failed. SQLite may still have made the Chat durable
    /// (the store treats this as CommitUnknown), so the fork is kept.
    CommitUnknown(AppError),
}

impl std::fmt::Display for ForkChatError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Refused(error) => error.fmt(formatter),
            Self::CommitUnknown(_) => formatter.write_str(FORK_UNFINISHED),
        }
    }
}

const FORK_UNFINISHED: &str = "The fork did not finish; try again";

impl Database {
    /// Creates the fork Chat for an already-forked native session in one
    /// transaction: Chat row and tab (directly above the parent), title,
    /// resumable head session and lineage. A changed source refuses it.
    pub(crate) fn create_fork_chat(
        &self,
        current_layout: &ProjectLayoutRequest,
        source: &ForkSource,
        external_session_id: &ExternalId,
    ) -> Result<String, ForkChatError> {
        let mut connection = self.write_connection().map_err(ForkChatError::Refused)?;
        let transaction = connection
            .transaction()
            .map_err(|error| ForkChatError::Refused(error.into()))?;
        let chat_id = stage_fork_chat(&transaction, current_layout, source, external_session_id)
            .map_err(ForkChatError::Refused)?;
        transaction
            .commit()
            .map_err(|error| ForkChatError::CommitUnknown(error.into()))?;
        Ok(chat_id)
    }
}

/// Every write of the fork Chat, inside the caller's transaction. Any error
/// here is a refusal: the transaction rolls back when it is dropped.
fn stage_fork_chat(
    transaction: &rusqlite::Transaction<'_>,
    current_layout: &ProjectLayoutRequest,
    source: &ForkSource,
    external_session_id: &ExternalId,
) -> AppResult<String> {
    let project = source.project_id.as_str();
    validate_chat_identifier("Project ID", project)?;
    if current_layout.project_id != project
        || !current_layout
            .tabs
            .iter()
            .any(|tab| tab.kind == "chat" && tab.id == source.chat_id.as_str())
    {
        return Err(AppError::InvalidRequest(
            "The forked Chat is not in the current Project layout".into(),
        ));
    }
    let chat_id = insert_layout_chat(
        transaction,
        current_layout,
        "Fork a Chat from its own active Project",
    )?;
    let titles = {
        let mut statement = transaction
            .prepare("SELECT title FROM chats WHERE project_id=?1 ORDER BY ordinal LIMIT 4097")?;
        statement
            .query_map([project], |row| row.get::<_, String>(0))?
            .collect::<Result<Vec<_>, _>>()?
    };
    let parent_title: String = transaction.query_row(
        "SELECT title FROM chats WHERE project_id=?1 AND id=?2",
        params![project, source.chat_id.as_str()],
        |row| row.get(0),
    )?;
    let title = fork_title(&parent_title, titles.iter().map(String::as_str));
    let now = now_millis();
    transaction.execute(
        "UPDATE chats SET title=?3,title_source=?4 WHERE project_id=?1 AND id=?2",
        params![
            project,
            chat_id,
            title,
            crate::chat_lifecycle::ChatTitleSource::Fork.as_str()
        ],
    )?;
    place_above_parent(
        transaction,
        project,
        &chat_id,
        source.chat_id.as_str(),
        &title,
    )?;
    let child = SageChatId::parse(chat_id.clone())
        .map_err(|_| AppError::InvalidRequest("fork Chat id is invalid".into()))?;
    install_fork(transaction, source, &child, external_session_id, now)
        .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
    Ok(chat_id)
}

/// Persists the current layout of the active Project and adds a new Chat to
/// it, within the tab limit; `inactive` is the refusal for another Project.
pub(crate) fn insert_layout_chat(
    transaction: &rusqlite::Transaction<'_>,
    current_layout: &ProjectLayoutRequest,
    inactive: &str,
) -> AppResult<String> {
    let project = current_layout.project_id.as_str();
    if current_layout.tabs.len() >= MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project layout has reached its {MAX_RESTORED_TABS}-tab limit"
        )));
    }
    let active: Option<String> = transaction.query_row(
        "SELECT active_project_id FROM app_state WHERE singleton=1",
        [],
        |row| row.get(0),
    )?;
    if active.as_deref() != Some(project) {
        return Err(AppError::InvalidRequest(inactive.into()));
    }
    persist_layout_in(transaction, current_layout)?;
    insert_new_chat(transaction, project, true, None)
}

/// Moves the new tab to the parent's position and shifts the parent and
/// every later tab down by one, through negative positions so the unique
/// `(project_id, position)` index never collides.
fn place_above_parent(
    transaction: &rusqlite::Transaction<'_>,
    project: &str,
    child: &str,
    parent: &str,
    title: &str,
) -> AppResult<()> {
    let parent_position: i64 = transaction.query_row(
        "SELECT position FROM workbench_tabs WHERE project_id=?1 AND id=?2",
        params![project, parent],
        |row| row.get(0),
    )?;
    transaction.execute(
        "UPDATE workbench_tabs SET position=-position-1
         WHERE project_id=?1 AND position>=?2 AND id<>?3",
        params![project, parent_position, child],
    )?;
    transaction.execute(
        "UPDATE workbench_tabs SET position=?3,title=?4 WHERE project_id=?1 AND id=?2",
        params![project, child, parent_position, title],
    )?;
    transaction.execute(
        "UPDATE workbench_tabs SET position=-position WHERE project_id=?1 AND position<0",
        [project],
    )?;
    Ok(())
}

pub(crate) fn stale(epoch: u64, expected: u64, action: &str) -> Result<(), String> {
    if epoch != expected || epoch >= 9_007_199_254_740_991 {
        return Err(format!("Project changed before {action}"));
    }
    Ok(())
}

/// Advances the Project transition epoch after a new Chat committed and
/// answers with the authoritative snapshot.
pub(crate) async fn committed_transition(
    state: &AppState,
    epoch: &mut u64,
) -> ProjectTransitionResult {
    *epoch += 1;
    crate::preview_lifecycle::on_committed_project_transition_epoch(*epoch);
    let snapshot = crate::resource_commands::build_bootstrap_snapshot(state, *epoch).await;
    ProjectTransitionResult {
        committed: true,
        transition_epoch: *epoch,
        snapshot: snapshot.as_ref().ok().cloned(),
        recovery_error: snapshot.err().map(|error| error.to_string()),
    }
}

/// Forks `chat_id` through its latest completed Turn `turn_id`. The native
/// fork runs on a blocking thread outside the Project transition lock (it can
/// wait on the harness); the lock is retaken and the epoch and source
/// re-validated before the fork Chat is committed.
#[tauri::command]
pub async fn fork_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    current_layout: ProjectLayoutRequest,
    chat_id: String,
    turn_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let project = SageProjectId::parse(current_layout.project_id.clone())
        .map_err(|error| error.to_string())?;
    let chat = SageChatId::parse(chat_id).map_err(|error| error.to_string())?;
    let turn = SageTurnId::parse(turn_id).map_err(|error| error.to_string())?;
    let source = {
        let epoch = state.project_transitions.lock().await;
        stale(*epoch, expected_transition_epoch, "forking the chat")?;
        fork_source(&state.database.connection(), &project, &chat, &turn)
            .map_err(|_| "This chat can't be forked from this answer".to_owned())?
    };
    let handle = app.clone();
    let native_source = source.clone();
    let external = tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<AppState>()
            .acp
            .fork_native_session(&native_source)
    })
    .await
    .map_err(|_| "The fork did not finish; try again".to_owned())?
    .map_err(|error| error.to_string())?;
    let mut epoch = state.project_transitions.lock().await;
    let committed = match stale(*epoch, expected_transition_epoch, "forking the chat") {
        Err(error) => ForkCommit::Refused(error),
        Ok(()) => {
            let database = state.database.clone();
            let layout = current_layout.clone();
            let (source, external) = (source.clone(), external.clone());
            match tauri::async_runtime::spawn_blocking(move || {
                database.create_fork_chat(&layout, &source, &external)
            })
            .await
            {
                Ok(Ok(_)) => ForkCommit::Committed,
                Ok(Err(error @ ForkChatError::Refused(_))) => {
                    ForkCommit::Refused(error.to_string())
                }
                // A failed COMMIT or a lost join may have landed.
                Ok(Err(ForkChatError::CommitUnknown(error))) => {
                    tracing::warn!(%error, "fork Chat commit failed; the native fork is kept");
                    ForkCommit::Unknown
                }
                Err(_) => ForkCommit::Unknown,
            }
        }
    };
    let release = release_after(&committed);
    let result = match committed {
        ForkCommit::Unknown => return Err(FORK_UNFINISHED.to_owned()),
        ForkCommit::Refused(error) => {
            drop(epoch);
            if let Some(committed) = release {
                release_fork(&app, source, external, committed);
            }
            return Err(error);
        }
        ForkCommit::Committed => committed_transition(&state, &mut epoch).await,
    };
    drop(epoch);
    if let Some(committed) = release {
        release_fork(&app, source, external, committed);
    }
    Ok(result)
}

/// What `fork_chat` learned about the fork Chat's commit.
#[derive(Debug)]
enum ForkCommit {
    Committed,
    /// Refused before the commit, with the reason for the user.
    Refused(String),
    /// A failed `COMMIT` or a lost join: the Chat may be durable.
    Unknown,
}

/// The native fork release a commit outcome allows: unsubscribe a committed
/// fork (`Some(true)`), delete a refused one (`Some(false)`), and release
/// nothing when the commit may have landed, since deleting the thread of a
/// durable fork Chat would lose its history.
fn release_after(commit: &ForkCommit) -> Option<bool> {
    match commit {
        ForkCommit::Committed => Some(true),
        ForkCommit::Refused(_) => Some(false),
        ForkCommit::Unknown => None,
    }
}

/// Lets the source runtime release the native fork once its Chat is
/// committed (it resumes in its own runtime) or refused (orphaned). It runs
/// detached: `fork_chat` answers as soon as the Chat commits, and a release
/// that meets a running Turn is deferred by the runtime to its next idle.
fn release_fork(app: &tauri::AppHandle, source: ForkSource, fork: ExternalId, committed: bool) {
    let handle = app.clone();
    drop(tauri::async_runtime::spawn_blocking(move || {
        handle
            .state::<AppState>()
            .acp
            .release_native_fork(&source, &fork, committed)
    }));
}

#[cfg(test)]
#[path = "chat_fork_tests.rs"]
mod tests;
