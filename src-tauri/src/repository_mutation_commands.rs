use std::time::Duration;

use tauri::State;

use crate::{
    domain::{
        GitFileMutationError, GitFileMutationErrorKind, GitFileMutationRequest,
        GitFileMutationResult,
    },
    error::{AppError, AppResult},
    repository,
    repository_mutation::{mutate_file, mutate_file_with_timeout},
    state::AppState,
    workspace::Workspace,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn mutate_git_file(
    state: State<'_, AppState>,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
) -> Result<GitFileMutationResult, String> {
    mutate_git_file_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(|error| error.to_string())
}

async fn mutate_git_file_inner(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitFileMutationResult> {
    mutate_git_file_inner_with_timeout_override(state, request, expected_transition_epoch, None)
        .await
}

#[cfg(test)]
async fn mutate_git_file_inner_with_timeout(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
    mutation_timeout: Duration,
) -> AppResult<GitFileMutationResult> {
    mutate_git_file_inner_with_timeout_override(
        state,
        request,
        expected_transition_epoch,
        Some(mutation_timeout),
    )
    .await
}

async fn mutate_git_file_inner_with_timeout_override(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
    mutation_timeout: Option<Duration>,
) -> AppResult<GitFileMutationResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git mutation Project identity exceeds its bounded contract".into(),
        ));
    }
    let transition_epoch = state.project_transitions.lock().await;
    if *transition_epoch != expected_transition_epoch {
        return Err(AppError::InvalidRequest(
            "Project changed before the requested Git mutation".into(),
        ));
    }
    let active_project_id = state
        .database
        .active_project_id()?
        .ok_or_else(|| AppError::InvalidRequest("no Project is active".into()))?;
    if active_project_id != request.project_id {
        return Err(AppError::InvalidRequest(
            "Git mutation does not belong to the active Project".into(),
        ));
    }
    let roots = state.database.root_records(&request.project_id)?;
    let target = roots
        .iter()
        .find(|root| root.id == request.resource.root_id)
        .ok_or_else(|| {
            AppError::InvalidRequest(
                "Git resource root does not belong to the active Project".into(),
            )
        })?;
    let workspace = Workspace::open(&target.project_id, &target.id, &target.absolute_path)?;
    let mutation_result = match mutation_timeout {
        Some(timeout) => {
            mutate_file_with_timeout(&workspace, &request.resource, request.action, timeout).await
        }
        None => mutate_file(&workspace, &request.resource, request.action).await,
    };
    let error = match mutation_result {
        Ok(()) => None,
        Err(AppError::GitEffectUnknown(message)) => Some(GitFileMutationError {
            kind: GitFileMutationErrorKind::EffectUnknown,
            message: format!(
                "{message}. Source Control was refreshed to show authoritative Git state."
            ),
        }),
        Err(error) => return Err(error),
    };

    let repository_roots = roots
        .into_iter()
        .map(|root| {
            let workspace = Workspace::open(&root.project_id, &root.id, &root.absolute_path)
                .map_err(|error| error.to_string());
            (root.id, workspace)
        })
        .collect();
    Ok(GitFileMutationResult {
        project_id: request.project_id,
        transition_epoch: *transition_epoch,
        repositories: repository::project_status(repository_roots).await,
        error,
    })
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use uuid::Uuid;

    use super::*;
    use crate::{
        domain::{GitFileMutationAction, GitFileMutationRequest},
        persistence::Database,
        state::AppState,
    };

    fn git(root: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn stage_command_returns_the_authoritative_epoch_bound_status() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = AppState::new(database);

            let result = mutate_git_file_inner(
                &state,
                GitFileMutationRequest {
                    project_id: project.id.clone(),
                    resource: crate::domain::ResourceRef {
                        root_id: root_id.clone(),
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                },
                0,
            )
            .await
            .unwrap();

            assert_eq!(result.project_id, project.id);
            assert_eq!(result.transition_epoch, 0);
            assert_eq!(result.repositories.roots.len(), 1);
            assert_eq!(result.repositories.roots[0].root_id, root_id);
            assert_eq!(
                result.repositories.roots[0].changes[0].index_status,
                "added"
            );
            assert!(result.error.is_none());
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn stale_epoch_rejects_before_any_git_mutation() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = AppState::new(database);
            *state.project_transitions.lock().await = 1;

            let error = mutate_git_file_inner(
                &state,
                GitFileMutationRequest {
                    project_id: project.id,
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                },
                0,
            )
            .await
            .unwrap_err();

            assert!(error.to_string().contains("Project changed"));
            let output = Command::new("git")
                .arg("-C")
                .arg(&root)
                .args(["status", "--porcelain=v2", "--", "new.txt"])
                .output()
                .unwrap();
            assert!(String::from_utf8(output.stdout).unwrap().starts_with("? "));
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn project_id_bound_accepts_exact_bytes_and_rejects_plus_one_before_lookup() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = AppState::new(database);

            let exact = mutate_git_file_inner(
                &state,
                GitFileMutationRequest {
                    project_id: "p".repeat(512),
                    resource: crate::domain::ResourceRef {
                        root_id: root_id.clone(),
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                },
                0,
            )
            .await
            .unwrap_err();
            assert!(exact.to_string().contains("active Project"));
            assert!(!exact.to_string().contains("bounded contract"));

            let plus_one = mutate_git_file_inner(
                &state,
                GitFileMutationRequest {
                    project_id: "p".repeat(513),
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                },
                0,
            )
            .await
            .unwrap_err();
            assert!(plus_one.to_string().contains("bounded contract"));
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn post_spawn_timeout_returns_typed_error_with_authoritative_status() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = AppState::new(database);

            let result = mutate_git_file_inner_with_timeout(
                &state,
                GitFileMutationRequest {
                    project_id: project.id,
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                },
                0,
                std::time::Duration::ZERO,
            )
            .await
            .unwrap();

            assert_eq!(
                result.error.as_ref().unwrap().kind,
                crate::domain::GitFileMutationErrorKind::EffectUnknown
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].index_status,
                "unchanged"
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].worktree_status,
                "untracked"
            );
            std::thread::sleep(std::time::Duration::from_millis(200));
            let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
            let later = repository::status(&workspace).await.unwrap();
            assert_eq!(later.changes[0].index_status, "unchanged");
            assert_eq!(later.changes[0].worktree_status, "untracked");
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }
}
