//! Commands that clear old data in bulk. Both release the affected Chats'
//! idle runtimes, then purge inside one Project transition.

use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;

use crate::{
    acp::service::explicit_close_deadline,
    chat_commands::{execute_chat_transition, failed_transition},
    data_cleanup::{CleanupScope, DataCleanupStore, MAX_RELEASED_CHATS},
    state::AppState,
};
use sage_acp_protocol::contract::{SageChatId, SageProjectId};
use sage_domain::project::ProjectTransitionResult;

/// Permanently deletes every archived Chat of a Project.
#[tauri::command]
pub async fn delete_archived_chats(
    state: State<'_, AppState>,
    project_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(clean_up(
        &state,
        project_id,
        CleanupScope::Archived,
        expected_transition_epoch,
    )
    .await)
}

/// Permanently deletes a user Project and all of its Chats. Files on disk
/// are not touched.
#[tauri::command]
pub async fn remove_project(
    state: State<'_, AppState>,
    project_id: String,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    Ok(clean_up(
        &state,
        project_id,
        CleanupScope::Project,
        expected_transition_epoch,
    )
    .await)
}

pub(crate) async fn clean_up(
    state: &AppState,
    project_id: String,
    scope: CleanupScope,
    expected_transition_epoch: u64,
) -> ProjectTransitionResult {
    // Admission runs before any runtime is released, so a refused cleanup
    // leaves every live Chat alone. Idle runtimes are parked next, and the
    // purge then refuses any that did not settle.
    // A stale request releases nothing.
    let epoch = *state.project_transitions.lock().await;
    if epoch != expected_transition_epoch {
        return failed_transition(epoch, "project changed before the requested cleanup");
    }
    let admitted = state
        .database
        .admit_cleanup(&project_id, scope)
        .and_then(|()| {
            state
                .database
                .cleanup_chat_ids(&project_id, scope, MAX_RELEASED_CHATS)
        });
    match admitted {
        Ok(chats) => release_runtimes(state, &project_id, &chats, scope).await,
        Err(error) => {
            let epoch = *state.project_transitions.lock().await;
            return failed_transition(epoch, error.public_message());
        }
    }
    let refusal = Arc::new(Mutex::new(None));
    let stopped = refusal.clone();
    let mut result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested cleanup",
        move |database| {
            let outcome = database.clean_up(&project_id, scope)?;
            *stopped.lock().unwrap_or_else(PoisonError::into_inner) =
                outcome.refusal.map(|error| error.public_message());
            Ok(())
        },
    )
    .await;
    if result.committed {
        // Chats already purged stay purged; the refusal that stopped the
        // rest is reported beside the committed snapshot.
        if let Some(message) = refusal
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .take()
        {
            result.recovery_error = Some(message);
        }
        crate::chat_archive_commands::forget_read_marks_of_deleted_chats(
            &state.database,
            &state.settings,
        );
        crate::settings_store::forget_removed_chat_access(&state.database, &state.settings);
    }
    result
}

/// Parks idle runtimes so the purge admits their Chats. A Chat that left the
/// scope meanwhile keeps its runtime; one whose runtime does not settle by
/// the shared deadline is refused by the purge.
async fn release_runtimes(state: &AppState, project: &str, chats: &[String], scope: CleanupScope) {
    let Ok(project_id) = SageProjectId::parse(project.to_owned()) else {
        return;
    };
    let deadline = explicit_close_deadline();
    for chat in chats {
        let Ok(chat_id) = SageChatId::parse(chat.clone()) else {
            continue;
        };
        if let Err(refusal) = state
            .acp
            .release_chat_runtime_until(&project_id, &chat_id, deadline, || {
                state.database.still_in_scope(project, chat, scope)
            })
            .await
        {
            tracing::debug!(?refusal, "Chat runtime not released before cleanup");
        }
    }
}
