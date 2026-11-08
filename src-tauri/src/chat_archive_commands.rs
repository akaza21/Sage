use tauri::{Manager, State};

use crate::{
    acp::{
        contract::{SageChatId, SageProjectId},
        service::ChatRuntimeRelease,
    },
    chat_commands::{execute_chat_transition, failed_transition},
    domain::{ChatDeleteRequest, ChatLifecycleRequest, ProjectTransitionResult},
    error::AppResult,
    state::AppState,
};

/// Archives a Chat and answers as soon as the archive commits; the Chat's
/// runtime is released afterwards on the blocking pool.
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
        tauri::async_runtime::spawn_blocking(move || {
            release_archived_runtime(&app.state::<AppState>(), &project, &chat);
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

/// Parks the Chat's live runtime through the lanes' explicit-close park,
/// handing this thread to Tokio's blocking pool while the harness exits.
fn release_chat_runtime(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<(), ChatRuntimeRelease> {
    let release = || state.acp.release_chat_runtime(project, chat);
    if tokio::runtime::Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(release)
    } else {
        release()
    }
}

/// Releases an archived Chat's runtime after its archive has answered. The
/// park is bounded by the explicit-close deadline; one that does not settle
/// is logged, and delete retries it. A Chat restored in the meantime keeps
/// its runtime.
pub(crate) fn release_archived_runtime(state: &AppState, project: &str, chat: &str) {
    let (Ok(project), Ok(chat)) = (
        SageProjectId::parse(project.to_owned()),
        SageChatId::parse(chat.to_owned()),
    ) else {
        return;
    };
    let archived = || {
        crate::chat_archive::is_archived_chat(
            &state.database.connection(),
            project.as_str(),
            chat.as_str(),
        )
    };
    match archived() {
        Ok(true) => {}
        Ok(false) => return,
        Err(error) => {
            tracing::warn!(%error, "archived Chat's runtime was not released");
            return;
        }
    }
    match release_chat_runtime(state, &project, &chat) {
        Ok(()) => {}
        Err(ChatRuntimeRelease::TurnRunning) if !matches!(archived(), Ok(true)) => {
            tracing::debug!("a restored Chat started a Turn before its runtime was released");
        }
        Err(ChatRuntimeRelease::TurnRunning) => {
            tracing::error!("archived Chat is running a Turn; archive admission was bypassed")
        }
        Err(refusal @ ChatRuntimeRelease::Unsettled) => {
            tracing::warn!(?refusal, "archived Chat's runtime is not yet released");
        }
    }
}

/// Releases an archived Chat's runtime before it is deleted, waiting out a
/// background release that is still parking it. Each attempt is bounded by
/// the explicit-close deadline, and so is the wait.
fn release_before_delete(
    state: &AppState,
    project: &SageProjectId,
    chat: &SageChatId,
) -> Result<(), ChatRuntimeRelease> {
    let deadline = std::time::Instant::now()
        + std::time::Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
    loop {
        match release_chat_runtime(state, project, chat) {
            Err(ChatRuntimeRelease::Unsettled) if std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(25));
            }
            outcome => return outcome,
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
        Ok(Some((project, chat))) => release_before_delete(state, &project, &chat)
            .err()
            .map(|refusal| refusal.message("deleting")),
        Ok(None) => None,
        Err(error) => Some(error.public_message()),
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
    if result.committed
        && let Err(error) = state.settings.mark_chat_unread(&chat_id)
    {
        // Startup drops marks whose Chat no longer exists.
        tracing::warn!(?error, "deleted Chat's read mark remains");
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
    let connection = database.connection();
    let outcome = settings.forget_chat_read_marks_unless(|chat| {
        Ok(connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE id=?1)",
            [chat],
            |row| row.get(0),
        )?)
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
