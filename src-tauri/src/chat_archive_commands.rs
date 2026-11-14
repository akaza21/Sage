use std::{
    sync::{Condvar, Mutex, PoisonError},
    time::Instant,
};

use sage_core::error::AppResult;
use tauri::{Manager, State};

use crate::{
    acp::{
        contract::{SageChatId, SageProjectId},
        service::{ChatRuntimeRelease, explicit_close_deadline},
    },
    chat_commands::{execute_chat_transition, failed_transition},
    domain::{ChatDeleteRequest, ChatLifecycleRequest, ProjectTransitionResult},
    state::AppState,
};

/// Archive releases still running after their archive answered. Exit waits
/// for them, within its shutdown budget, before shutting the lanes down.
#[derive(Debug, Default)]
pub struct ArchiveReleases {
    running: Mutex<usize>,
    settled: Condvar,
}

impl ArchiveReleases {
    fn begin(&self) {
        *self.running.lock().unwrap_or_else(PoisonError::into_inner) += 1;
    }

    fn end(&self) {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        *running = running.saturating_sub(1);
        self.settled.notify_all();
    }

    /// Waits until no archive release is running or `deadline` passes, and
    /// answers whether they all settled.
    pub(crate) fn wait_settled(&self, deadline: Instant) -> bool {
        let mut running = self.running.lock().unwrap_or_else(PoisonError::into_inner);
        while *running > 0 {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return false;
            }
            running = self
                .settled
                .wait_timeout(running, left)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
        true
    }
}

/// Ends one tracked archive release however its task ends.
struct ReleaseRunning<'a>(&'a ArchiveReleases);

impl Drop for ReleaseRunning<'_> {
    fn drop(&mut self) {
        self.0.end();
    }
}

/// Archives a Chat and answers as soon as the archive commits; the Chat's
/// runtime is released afterwards in the background.
#[tauri::command]
pub async fn archive_chat(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let (project, chat) = (request.project_id.clone(), request.chat_id.clone());
    let result = archive_chat_inner(&state, request, expected_transition_epoch).await;
    if result.committed {
        // Counted before the task starts, so an exit requested now waits.
        state.archive_releases.begin();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _running = ReleaseRunning(&state.archive_releases);
            release_archived_runtime(&state, &project, &chat).await;
        });
    }
    Ok(result)
}

#[tauri::command]
pub async fn restore_chat(
    state: State<'_, AppState>,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(restore_chat_inner(&state, request, expected_transition_epoch).await)
}

/// Permanently deletes an archived Chat and forgets its read mark.
#[tauri::command]
pub async fn delete_chat(
    state: State<'_, AppState>,
    request: ChatDeleteRequest,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(delete_chat_inner(&state, request, expected_transition_epoch).await)
}

/// Whether the Chat is still archived; a failed lookup answers no, so the
/// runtime is kept.
fn still_archived(state: &AppState, project: &SageProjectId, chat: &SageChatId) -> bool {
    crate::chat_archive::is_archived_chat(
        &state.database.connection(),
        project.as_str(),
        chat.as_str(),
    )
    .is_ok_and(|archived| archived)
}

/// Parks an archived Chat's live runtime through the lanes' explicit-close
/// park within `deadline`. The lane is claimed before the Chat is checked
/// as still archived, so a Chat restored meanwhile keeps its runtime.
async fn release_until(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
    deadline: Instant,
) -> Result<(), ChatRuntimeRelease> {
    state
        .acp
        .release_chat_runtime_until(project, chat, deadline, || {
            still_archived(state, project, chat)
        })
        .await
}

/// Releases an archived Chat's runtime after its archive has answered,
/// within the explicit-close deadline. An outcome other than a release is
/// logged; delete releases the runtime again before it deletes the Chat.
pub(crate) async fn release_archived_runtime(state: &AppState, project: &str, chat: &str) {
    let (Ok(project), Ok(chat)) = (
        SageProjectId::parse(project.to_owned()),
        SageChatId::parse(chat.to_owned()),
    ) else {
        return;
    };
    match release_until(state, &project, &chat, explicit_close_deadline()).await {
        Ok(()) => {}
        Err(ChatRuntimeRelease::TurnRunning) if !still_archived(state, &project, &chat) => {
            tracing::debug!("a restored Chat started a Turn before its runtime was released");
        }
        Err(ChatRuntimeRelease::TurnRunning) => {
            tracing::error!("archived Chat is running a Turn; archive admission was bypassed")
        }
        Err(refusal) => {
            tracing::warn!(?refusal, "archived Chat's runtime is not yet released");
        }
    }
}

/// The Chat's typed identity when it is an archived Chat of that Project.
fn archived_identity(
    state: &AppState,
    request: &ChatDeleteRequest,
) -> AppResult<Option<(SageProjectId, SageChatId)>> {
    let archived = crate::chat_archive::is_archived_chat(
        &state.database.connection(),
        &request.project_id,
        &request.chat_id,
    )?;
    Ok(archived
        .then(|| {
            Some((
                SageProjectId::parse(request.project_id.clone()).ok()?,
                SageChatId::parse(request.chat_id.clone()).ok()?,
            ))
        })
        .flatten())
}

pub(crate) async fn delete_chat_inner(
    state: &AppState,
    request: ChatDeleteRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    // An archived Chat may still hold an idle runtime from before it was
    // archived, or one a background release is still parking; release it
    // instead of refusing. Only an archived Chat's runtime is touched;
    // anything else is refused below.
    let refusal = match archived_identity(state, &request) {
        Ok(Some((project, chat))) => {
            release_until(state, &project, &chat, explicit_close_deadline())
                .await
                .err()
                .map(|refusal| refusal.message("deleting"))
        }
        Ok(None) => None,
        Err(error) => {
            tracing::warn!(%error, "archived Chat lookup failed before delete");
            Some(error.public_message())
        }
    };
    if let Some(message) = refusal {
        let epoch = *state.project_transitions.lock().await;
        return failed_transition(epoch, message);
    }
    let chat_id = request.chat_id.clone();
    let result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat delete",
        move |database| database.delete_archived_chat(&request),
    )
    .await;
    if result.committed {
        if let Err(error) = state.settings.mark_chat_unread(&chat_id) {
            // Startup drops marks whose Chat no longer exists.
            tracing::warn!(?error, "deleted Chat's read mark remains");
        }
        // The delete removed the Chat's sessions; their access choices go
        // with them. A failure is logged there, and startup retries it.
        crate::settings_store::forget_removed_chat_access(&state.database, &state.settings);
    }
    result
}

/// Drops the read marks of Chats that no longer exist, which a crash between
/// a delete and its mark removal leaves behind. Marks are view state, so a
/// failure is only logged.
pub(crate) fn forget_read_marks_of_deleted_chats(
    database: &crate::persistence::Database,
    settings: &crate::settings_store::SettingsStore,
) {
    // Each store's lock is taken alone: collect, check, then forget.
    let outcome = settings.chat_read_mark_ids().and_then(|marked| {
        let connection = database.connection();
        let mut missing = Vec::new();
        for chat in marked {
            let exists: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM chats WHERE id=?1)",
                [&chat],
                |row| row.get(0),
            )?;
            if !exists {
                missing.push(chat);
            }
        }
        drop(connection);
        settings.forget_chat_read_marks(&missing)
    });
    match outcome {
        Ok(0) => {}
        Ok(forgotten) => tracing::info!(forgotten, "forgot read marks of deleted Chats"),
        Err(error) => tracing::warn!(%error, "deleted Chats' read marks remain"),
    }
}

/// Archives the Chat and settles its Chat-scoped state. The caller releases
/// its runtime afterwards with `release_archived_runtime`, so archive never
/// waits on a harness exiting.
pub(crate) async fn archive_chat_inner(
    state: &AppState,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    let project_id = request.project_id.clone();
    let chat_id = request.chat_id.clone();
    let result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat archive",
        move |database| database.archive_chat(&request),
    )
    .await;
    if result.committed {
        // An archived chat is off the rail, so its read mark goes with it; a
        // restored chat starts without one.
        if let Err(error) = state.settings.mark_chat_unread(&chat_id) {
            tracing::warn!(?error, "archived Chat keeps its read mark");
        }
    }
    if result.committed
        && let (Ok(project), Ok(chat)) =
            (SageProjectId::parse(project_id), SageChatId::parse(chat_id))
    {
        if let Err(error) = state.terminals.close_chat(&project, &chat) {
            tracing::warn!(?error, "terminal Chat-close cleanup remains unsettled");
        }
        state
            .acp
            .file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_chat(project.as_str(), chat.as_str());
        state.acp.revoke_browser_chat(&project, &chat);
    }
    result
}

async fn restore_chat_inner(
    state: &AppState,
    request: ChatLifecycleRequest,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested Chat restore",
        move |database| database.restore_chat(&request),
    )
    .await
}

#[cfg(test)]
#[path = "chat_archive_commands_tests.rs"]
mod tests;
