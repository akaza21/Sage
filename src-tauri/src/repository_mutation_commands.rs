#[cfg(test)]
use std::time::Duration;

use tauri::State;

use crate::{
    domain::{
        GitFileMutationCommandError, GitFileMutationError, GitFileMutationErrorKind,
        GitFileMutationRequest, GitFileMutationResult, GitProcessState,
    },
    error::{AppError, AppResult, GitContainmentState},
    repository,
    repository_mutation::{mutate_file, mutate_file_with_process_policy},
    state::AppState,
    workspace::Workspace,
};

const MAX_GIT_PROJECT_ID_BYTES: usize = 512;

#[tauri::command]
pub async fn mutate_git_file(
    state: State<'_, AppState>,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
) -> Result<GitFileMutationResult, GitFileMutationCommandError> {
    mutate_git_file_inner(&state, request, expected_transition_epoch)
        .await
        .map_err(git_mutation_command_error)
}

fn git_mutation_command_error(error: AppError) -> GitFileMutationCommandError {
    match error {
        AppError::GitContainment { state, message } => GitFileMutationCommandError::Containment {
            process_state: match state {
                GitContainmentState::TerminatedUnreaped => GitProcessState::TerminatedUnreaped,
                GitContainmentState::PotentiallyRunning => GitProcessState::PotentiallyRunning,
            },
            message,
        },
        error => GitFileMutationCommandError::Failed {
            message: error.to_string(),
        },
    }
}

async fn mutate_git_file_inner(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
) -> AppResult<GitFileMutationResult> {
    mutate_git_file_inner_with_policy_override(state, request, expected_transition_epoch, None)
        .await
}

#[cfg(test)]
async fn mutate_git_file_inner_with_process_policy(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<GitFileMutationResult> {
    mutate_git_file_inner_with_policy_override(
        state,
        request,
        expected_transition_epoch,
        Some(process_policy),
    )
    .await
}

async fn mutate_git_file_inner_with_policy_override(
    state: &AppState,
    request: GitFileMutationRequest,
    expected_transition_epoch: u64,
    process_policy: Option<repository::GitProcessPolicy>,
) -> AppResult<GitFileMutationResult> {
    if request.project_id.is_empty() || request.project_id.len() > MAX_GIT_PROJECT_ID_BYTES {
        return Err(AppError::InvalidRequest(
            "Git mutation Project identity exceeds its bounded contract".into(),
        ));
    }
    if request.action == crate::domain::GitFileMutationAction::Discard && !request.discard_confirmed
    {
        return Err(AppError::InvalidRequest(
            "Git discard requires explicit confirmation".into(),
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
    let mutation_result = match process_policy {
        Some(policy) => {
            mutate_file_with_process_policy(&workspace, &request.resource, request.action, policy)
                .await
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
    use std::{fs, path::Path, process::Command, sync::Arc};

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
                    discard_confirmed: false,
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
    fn discard_command_requires_confirmation_then_returns_authoritative_status() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            git(&root, &["config", "user.email", "sage@example.invalid"]);
            git(&root, &["config", "user.name", "Sage Test"]);
            fs::write(root.join("tracked.txt"), "base\n").unwrap();
            git(&root, &["add", "--", "tracked.txt"]);
            git(&root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("tracked.txt"), "keep changed\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = AppState::new(database);

            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id,
                    relative_path: "tracked.txt".into(),
                },
                action: GitFileMutationAction::Discard,
                discard_confirmed: false,
            };
            let error = mutate_git_file_inner(&state, request.clone(), 0)
                .await
                .unwrap_err();

            assert!(error.to_string().contains("explicit confirmation"));
            assert_eq!(
                fs::read_to_string(root.join("tracked.txt")).unwrap(),
                "keep changed\n"
            );
            let result = mutate_git_file_inner(
                &state,
                GitFileMutationRequest {
                    discard_confirmed: true,
                    ..request
                },
                0,
            )
            .await
            .unwrap();
            assert!(result.error.is_none());
            assert!(result.repositories.roots[0].changes.is_empty());
            assert_eq!(
                fs::read_to_string(root.join("tracked.txt")).unwrap(),
                "base\n"
            );
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
                        root_id: root_id.clone(),
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Discard,
                    discard_confirmed: true,
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
                    discard_confirmed: false,
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
                    discard_confirmed: false,
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
    fn finite_deadline_after_real_spawn_returns_typed_error_with_authoritative_status() {
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
            let state = Arc::new(AppState::new(database));

            let (policy, mut deadline_gate) =
                repository::GitProcessPolicy::with_deadline_wait_gate(Duration::from_millis(50));
            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: "new.txt".into(),
                },
                action: GitFileMutationAction::Stage,
                discard_confirmed: false,
            };
            let mutation_state = Arc::clone(&state);
            let mutation = tokio::spawn(async move {
                mutate_git_file_inner_with_process_policy(&mutation_state, request, 0, policy).await
            });
            deadline_gate.wait_for_entry().await.unwrap();
            let result = mutation.await.unwrap().unwrap();
            deadline_gate.release();

            assert_eq!(
                result.error.as_ref().unwrap().kind,
                crate::domain::GitFileMutationErrorKind::EffectUnknown
            );
            let authoritative_changes =
                serde_json::to_value(&result.repositories.roots[0].changes).unwrap();
            let workspace = Workspace::open("project-a", &root_id, &root).unwrap();
            let later = repository::status(&workspace).await.unwrap();
            assert_eq!(
                serde_json::to_value(later.changes).unwrap(),
                authoritative_changes
            );
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn discard_timeout_refreshes_authoritative_status_and_never_changes_later() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            git(&root, &["config", "user.email", "sage@example.invalid"]);
            git(&root, &["config", "user.name", "Sage Test"]);
            fs::write(root.join("tracked.txt"), "base\n").unwrap();
            git(&root, &["add", "--", "tracked.txt"]);
            git(&root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("tracked.txt"), "changed\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let root_id = snapshot.project.unwrap().roots[0].id.clone();
            let state = Arc::new(AppState::new(database));

            let (policy, mut deadline_gate) =
                repository::GitProcessPolicy::with_deadline_wait_gate(Duration::from_millis(50));
            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id: root_id.clone(),
                    relative_path: "tracked.txt".into(),
                },
                action: GitFileMutationAction::Discard,
                discard_confirmed: true,
            };
            let mutation_state = Arc::clone(&state);
            let mutation = tokio::spawn(async move {
                mutate_git_file_inner_with_process_policy(&mutation_state, request, 0, policy).await
            });
            deadline_gate.wait_for_entry().await.unwrap();
            let result = mutation.await.unwrap().unwrap();
            deadline_gate.release();

            assert_eq!(
                result.error.as_ref().unwrap().kind,
                crate::domain::GitFileMutationErrorKind::EffectUnknown
            );
            let authoritative_changes =
                serde_json::to_value(&result.repositories.roots[0].changes).unwrap();
            let workspace = Workspace::open("project-a", &root_id, &root).unwrap();
            let later = repository::status(&workspace).await.unwrap();
            assert_eq!(
                serde_json::to_value(later.changes).unwrap(),
                authoritative_changes
            );
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn nonconvergent_reap_returns_containment_without_a_final_projection() {
        let _serial = crate::test_wait::serial_process_guard();
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

            let error = mutate_git_file_inner_with_process_policy(
                &state,
                GitFileMutationRequest {
                    project_id: project.id,
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                    discard_confirmed: false,
                },
                0,
                repository::GitProcessPolicy::nonconvergent_reap(Duration::ZERO),
            )
            .await
            .unwrap_err();

            assert!(matches!(
                error,
                AppError::GitContainment {
                    state: crate::error::GitContainmentState::TerminatedUnreaped,
                    ..
                }
            ));
            std::thread::sleep(Duration::from_millis(200));
            let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
            let later = repository::status(&workspace).await.unwrap();
            assert_eq!(later.changes[0].index_status, "unchanged");
            assert_eq!(later.changes[0].worktree_status, "untracked");
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn inner_failure_after_proven_reap_returns_effect_unknown_and_refreshes() {
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

            let result = mutate_git_file_inner_with_process_policy(
                &state,
                GitFileMutationRequest {
                    project_id: project.id,
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                    discard_confirmed: false,
                },
                0,
                repository::GitProcessPolicy::post_reap_failure(crate::test_wait::BUDGET),
            )
            .await
            .unwrap();

            assert_eq!(
                result.error.as_ref().unwrap().kind,
                crate::domain::GitFileMutationErrorKind::EffectUnknown
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].index_status,
                "added"
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].worktree_status,
                "unchanged"
            );
            std::thread::sleep(Duration::from_millis(200));
            let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
            let later = repository::status(&workspace).await.unwrap();
            assert_eq!(later.changes[0].index_status, "added");
            assert_eq!(later.changes[0].worktree_status, "unchanged");
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn consumed_stdout_reader_failure_is_effect_unknown_and_refreshes_without_panicking() {
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

            let result = mutate_git_file_inner_with_process_policy(
                &state,
                GitFileMutationRequest {
                    project_id: project.id,
                    resource: crate::domain::ResourceRef {
                        root_id,
                        relative_path: "new.txt".into(),
                    },
                    action: GitFileMutationAction::Stage,
                    discard_confirmed: false,
                },
                0,
                repository::GitProcessPolicy::stdout_reader_failure(crate::test_wait::BUDGET),
            )
            .await
            .unwrap();

            assert_eq!(
                result.error.as_ref().unwrap().kind,
                crate::domain::GitFileMutationErrorKind::EffectUnknown
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].index_status,
                "added"
            );
            assert_eq!(
                result.repositories.roots[0].changes[0].worktree_status,
                "unchanged"
            );
            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn potentially_running_cleanup_blocks_mutations_until_bounded_reaper_proves_exit() {
        let _serial = crate::test_wait::serial_process_guard();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id: snapshot.project.unwrap().roots[0].id.clone(),
                    relative_path: "new.txt".into(),
                },
                action: GitFileMutationAction::Stage,
                discard_confirmed: false,
            };
            let state = AppState::new(database);
            let (cleanup_policy, reaper) =
                repository::GitProcessPolicy::potentially_running(Duration::ZERO);

            let first = mutate_git_file_inner_with_process_policy(
                &state,
                request.clone(),
                0,
                cleanup_policy,
            )
            .await
            .unwrap_err();
            let first = serde_json::to_value(git_mutation_command_error(first)).unwrap();
            assert_eq!(first["kind"], "containment");
            assert_eq!(first["processState"], "potentiallyRunning");
            assert!(first.get("repositories").is_none());

            let blocked = mutate_git_file_inner_with_process_policy(
                &state,
                request.clone(),
                0,
                reaper.mutation_policy(crate::test_wait::BUDGET, Duration::from_millis(10)),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                blocked,
                AppError::GitContainment {
                    state: crate::error::GitContainmentState::PotentiallyRunning,
                    ..
                }
            ));

            reaper.release();
            reaper
                .wait_for_exit(crate::test_wait::BUDGET)
                .await
                .unwrap();
            let recovered = mutate_git_file_inner_with_process_policy(
                &state,
                request,
                0,
                reaper.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET),
            )
            .await
            .unwrap();
            assert!(recovered.error.is_none());
            assert_eq!(
                recovered.repositories.roots[0].changes[0].index_status,
                "added"
            );

            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn successful_kill_keeps_mutation_admission_charged_until_wait_proves_exit() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id: snapshot.project.unwrap().roots[0].id.clone(),
                    relative_path: "new.txt".into(),
                },
                action: GitFileMutationAction::Stage,
                discard_confirmed: false,
            };
            let state = AppState::new(database);
            let (cleanup_policy, reaper) =
                repository::GitProcessPolicy::terminated_unreaped_withheld_wait(Duration::ZERO);

            let first = mutate_git_file_inner_with_process_policy(
                &state,
                request.clone(),
                0,
                cleanup_policy,
            )
            .await
            .unwrap_err();
            assert!(matches!(
                first,
                AppError::GitContainment {
                    state: crate::error::GitContainmentState::TerminatedUnreaped,
                    ..
                }
            ));

            let blocked = mutate_git_file_inner_with_process_policy(
                &state,
                request.clone(),
                0,
                reaper.mutation_policy(crate::test_wait::BUDGET, Duration::from_millis(10)),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                blocked,
                AppError::GitContainment {
                    state: crate::error::GitContainmentState::TerminatedUnreaped,
                    ..
                }
            ));

            reaper.release_wait();
            reaper
                .wait_for_exit(crate::test_wait::BUDGET)
                .await
                .unwrap();
            let recovered = mutate_git_file_inner_with_process_policy(
                &state,
                request,
                0,
                reaper.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET),
            )
            .await
            .unwrap();
            assert!(recovered.error.is_none());

            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn exhausted_real_reapers_quarantine_exact_eight_and_keep_admission_charged() {
        let _serial = crate::test_wait::serial_process_guard();
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
            let root = base.join("repo");
            fs::create_dir_all(&root).unwrap();
            git(&root, &["init", "-q"]);
            fs::write(root.join("new.txt"), "new\n").unwrap();
            let database = Database::open(&base.join("sage.db")).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
            let snapshot = database.bootstrap_snapshot().unwrap();
            let request = GitFileMutationRequest {
                project_id: project.id,
                resource: crate::domain::ResourceRef {
                    root_id: snapshot.project.unwrap().roots[0].id.clone(),
                    relative_path: "new.txt".into(),
                },
                action: GitFileMutationAction::Stage,
                discard_confirmed: false,
            };
            let state = AppState::new(database);
            let harness = repository::GitProcessTestHarness::new();

            for _ in 0..8 {
                let (policy, reaper) = harness.exhausting_reaper_policy(Duration::ZERO);
                let error =
                    mutate_git_file_inner_with_process_policy(&state, request.clone(), 0, policy)
                        .await
                        .unwrap_err();
                assert!(matches!(
                    error,
                    AppError::GitContainment {
                        state: crate::error::GitContainmentState::TerminatedUnreaped,
                        ..
                    }
                ));
                reaper
                    .wait_for_quarantine(crate::test_wait::BUDGET)
                    .await
                    .unwrap();
                assert_eq!(reaper.wait_attempts(), 40);
            }

            let saturated = harness.observation();
            assert_eq!(saturated.unconfirmed_exits, 8);
            assert_eq!(saturated.quarantined_owners, 8);
            assert_eq!(saturated.active_reapers, 0);
            assert_eq!(saturated.available_reaper_permits, 0);
            assert!(matches!(
                harness.probe_reaper_owner(Duration::from_millis(10)).await,
                Err(AppError::ResourceLimit(_))
            ));

            let blocked = mutate_git_file_inner_with_process_policy(
                &state,
                request.clone(),
                0,
                harness.mutation_policy(crate::test_wait::BUDGET, Duration::from_millis(10)),
            )
            .await
            .unwrap_err();
            assert!(matches!(
                blocked,
                AppError::GitContainment {
                    state: crate::error::GitContainmentState::TerminatedUnreaped,
                    ..
                }
            ));

            harness
                .reap_quarantine(crate::test_wait::BUDGET)
                .await
                .unwrap();
            let reset = harness.observation();
            assert_eq!(reset.unconfirmed_exits, 0);
            assert_eq!(reset.quarantined_owners, 0);
            assert_eq!(reset.active_reapers, 0);
            assert_eq!(reset.available_reaper_permits, 8);
            harness
                .reap_quarantine(crate::test_wait::BUDGET)
                .await
                .unwrap();
            assert_eq!(harness.observation().unconfirmed_exits, 0);

            let recovered = mutate_git_file_inner_with_process_policy(
                &state,
                request,
                0,
                harness.mutation_policy(crate::test_wait::BUDGET, crate::test_wait::BUDGET),
            )
            .await
            .unwrap();
            assert!(recovered.error.is_none());

            drop(state);
            fs::remove_dir_all(base).unwrap();
        });
    }

    #[test]
    fn containment_command_rejection_is_typed_and_carries_no_projection() {
        let encoded = serde_json::to_value(git_mutation_command_error(AppError::GitContainment {
            state: crate::error::GitContainmentState::TerminatedUnreaped,
            message: "Git termination is proven, but reap did not converge".into(),
        }))
        .unwrap();

        assert_eq!(
            encoded,
            serde_json::json!({
                "kind": "containment",
                "processState": "terminatedUnreaped",
                "message": "Git termination is proven, but reap did not converge"
            })
        );
        assert!(encoded.get("repositories").is_none());
    }
}
