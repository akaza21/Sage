use sage_core::error::{AppError, AppResult, GitContainmentState};
use tauri::State;

use crate::project_registry::ProjectRegistryStore;
use crate::{repository_remote_transport, state::AppState, workspace::Workspace};
use sage_domain::file_operations::{
    GitFileMutationCommandError, GitRepositoryRemoteTransportRequest,
    GitRepositoryRemoteTransportResult,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;
const MAX_GIT_ROOT_ID_BYTES: usize = 512;
const MAX_GIT_REMOTE_NAME_BYTES: usize = 255;

#[tauri::command]
pub async fn fetch_git_remote(
    state: State<'_, AppState>,
    request: GitRepositoryRemoteTransportRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryRemoteTransportResult, GitFileMutationCommandError> {
    transport_git_remote_inner(
        &state,
        request,
        expected_transition_epoch,
        RemoteTransport::Fetch,
    )
    .await
    .map_err(remote_transport_command_error)
}

#[tauri::command]
pub async fn push_git_remote(
    state: State<'_, AppState>,
    request: GitRepositoryRemoteTransportRequest,
    expected_transition_epoch: u64,
) -> Result<GitRepositoryRemoteTransportResult, GitFileMutationCommandError> {
    transport_git_remote_inner(
        &state,
        request,
        expected_transition_epoch,
        RemoteTransport::Push,
    )
    .await
    .map_err(remote_transport_command_error)
}

enum RemoteTransport {
    Fetch,
    Push,
}

impl RemoteTransport {
    fn label(&self) -> &'static str {
        match self {
            Self::Fetch => "fetch",
            Self::Push => "push",
        }
    }
}

fn remote_transport_command_error(error: AppError) -> GitFileMutationCommandError {
    match error {
        AppError::GitContainment { state, .. } => GitFileMutationCommandError::Containment {
            process_state: match state {
                GitContainmentState::TerminatedUnreaped => {
                    sage_domain::file_operations::GitProcessState::TerminatedUnreaped
                }
                GitContainmentState::PotentiallyRunning => {
                    sage_domain::file_operations::GitProcessState::PotentiallyRunning
                }
            },
            message: crate::repository_public_error::containment().into(),
        },
        error => GitFileMutationCommandError::Failed {
            message: crate::repository_public_error::remote_transport(&error),
        },
    }
}

async fn transport_git_remote_inner(
    state: &AppState,
    request: GitRepositoryRemoteTransportRequest,
    expected_transition_epoch: u64,
    transport: RemoteTransport,
) -> AppResult<GitRepositoryRemoteTransportResult> {
    let action = transport.label();
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(format!(
            "Git {action} Project identity exceeds its bounded contract"
        )));
    }
    if request.root_id.is_empty() || request.root_id.len() > MAX_GIT_ROOT_ID_BYTES {
        return Err(AppError::InvalidRequest(format!(
            "Git {action} root identity exceeds its bounded contract"
        )));
    }
    if request.remote.is_empty() || request.remote.len() > MAX_GIT_REMOTE_NAME_BYTES {
        return Err(AppError::InvalidRequest(
            "Git fetch and push require an exact listed remote name".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(format!(
            "Project changed before the requested Git {action}"
        )));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(format!(
            "Git {action} does not belong to the active Project"
        )));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(format!(
                "Git {action} root does not belong to the active Project"
            ))
        })?;
    let workspace = Workspace::open(&target.project_id, &target.id, &target.absolute_path)?;
    let repository_roots = roots
        .iter()
        .map(|root| {
            let workspace = if root.id == target.id {
                Ok(workspace.clone())
            } else {
                Workspace::open(&root.project_id, &root.id, &root.absolute_path)
            };
            (root.id.clone(), workspace)
        })
        .collect();
    let transport_result = match transport {
        RemoteTransport::Fetch => {
            repository_remote_transport::fetch_listed_remote(&workspace, &request.remote).await
        }
        RemoteTransport::Push => {
            repository_remote_transport::push_listed_remote(&workspace, &request.remote).await
        }
    };
    let error = match transport_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(_)) => Some(sage_domain::file_operations::GitFileMutationError {
            kind: sage_domain::file_operations::GitFileMutationErrorKind::EffectUnknown,
            message: "The Git fetch or push outcome is uncertain. Source Control recovery is incomplete; refresh to reconcile authoritative Git state."
                .into(),
        }),
        Err(error) => return Err(error),
    };
    Ok(GitRepositoryRemoteTransportResult {
        project_id: request.project_id,
        root_id: request.root_id,
        transition_epoch: *transition_epoch,
        repositories: crate::repository_index_status::project_status(repository_roots).await,
        error,
    })
}
