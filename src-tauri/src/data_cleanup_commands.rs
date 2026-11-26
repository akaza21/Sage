//! Commands that clear old data in bulk. Each releases the affected Chats'
//! idle runtimes, then purges inside one Project transition.

use std::sync::{Arc, Mutex, PoisonError};

use tauri::State;

use crate::{
    acp::service::explicit_close_deadline,
    chat_commands::{execute_chat_transition, failed_transition},
    data_cleanup::{CleanupOutcome, CleanupScope, DataCleanupStore, MAX_RELEASED_CHATS},
    data_cleanup_stale::{StaleChatStore, stale_scope},
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
        vec![project_id],
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
        vec![project_id],
        CleanupScope::Project,
        expected_transition_epoch,
    )
    .await)
}

/// How many Chats `delete_stale_chats` would remove at `cutoff` (Unix ms).
#[tauri::command]
pub async fn count_stale_chats(state: State<'_, AppState>, cutoff: i64) -> Result<u64, String> {
    stale_scope(cutoff, sage_core::time::now_millis())
        .and_then(|scope| state.database.stale_chat_count(scope))
        .map_err(|error| error.public_message())
}

/// Permanently deletes, in every Project, the Chats last changed before
/// `cutoff` (Unix ms). Pinned Chats and Chats running a Turn stay.
#[tauri::command]
pub async fn delete_stale_chats(
    state: State<'_, AppState>,
    cutoff: i64,
    expected_transition_epoch: u64,
) -> Result<ProjectTransitionResult, String> {
    let scoped = stale_scope(cutoff, sage_core::time::now_millis())
        .and_then(|scope| Ok((scope, state.database.stale_projects(scope)?)));
    Ok(match scoped {
        Ok((scope, projects)) => clean_up(&state, projects, scope, expected_transition_epoch).await,
        Err(error) => {
            let epoch = *state.project_transitions.lock().await;
            failed_transition(epoch, error.public_message())
        }
    })
}

pub(crate) async fn clean_up(
    state: &AppState,
    projects: Vec<String>,
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
    let mut release = Vec::new();
    for project in &projects {
        let left = (MAX_RELEASED_CHATS - release.len() as i64).max(0);
        let admitted = state
            .database
            .admit_cleanup(project, scope)
            .and_then(|()| state.database.cleanup_chat_ids(project, scope, left));
        match admitted {
            Ok(chats) => release.extend(chats.into_iter().map(|chat| (project.clone(), chat))),
            Err(error) => {
                let epoch = *state.project_transitions.lock().await;
                return failed_transition(epoch, error.public_message());
            }
        }
    }
    release_runtimes(state, &release, scope).await;
    let refusal = Arc::new(Mutex::new(None));
    let stopped = refusal.clone();
    let mut result = execute_chat_transition(
        state,
        expected_transition_epoch,
        "project changed before the requested cleanup",
        move |database| {
            let mut deleted = 0;
            for project in &projects {
                let outcome = match database.clean_up(project, scope) {
                    Ok(outcome) => outcome,
                    // Nothing went yet: the whole cleanup is refused.
                    Err(error) if deleted == 0 => return Err(error),
                    Err(error) => CleanupOutcome {
                        deleted: 0,
                        refusal: Some(error),
                    },
                };
                deleted += outcome.deleted;
                if let Some(error) = outcome.refusal {
                    *stopped.lock().unwrap_or_else(PoisonError::into_inner) =
                        Some(error.public_message());
                    break;
                }
            }
            tracing::info!(deleted, ?scope, "old data cleaned up");
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
            // A transition's own recovery error stays first.
            result.recovery_error = Some(match result.recovery_error.take() {
                Some(earlier) => format!("{earlier} {message}"),
                None => message,
            });
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
async fn release_runtimes(state: &AppState, chats: &[(String, String)], scope: CleanupScope) {
    let deadline = explicit_close_deadline();
    for (project, chat) in chats {
        let (Ok(project_id), Ok(chat_id)) = (
            SageProjectId::parse(project.clone()),
            SageChatId::parse(chat.clone()),
        ) else {
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
