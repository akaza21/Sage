use std::{fs, path::Path, process::Command};

use uuid::Uuid;

use super::*;
use crate::{persistence::Database, state::AppState};

struct TwoRootProject {
    base: std::path::PathBuf,
    root_a: std::path::PathBuf,
    root_b: std::path::PathBuf,
}

impl TwoRootProject {
    fn new() -> Self {
        let base =
            crate::test_paths::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
        let root_a = base.join("repo-a");
        let root_b = base.join("repo-b");
        for root in [&root_a, &root_b] {
            fs::create_dir_all(root).unwrap();
            git(root, &["init", "-q"]);
            git(root, &["config", "user.email", "sage@example.invalid"]);
            git(root, &["config", "user.name", "Sage Test"]);
            git(root, &["config", "core.autocrlf", "false"]);
            fs::write(root.join("same.txt"), "base\n").unwrap();
            git(root, &["add", "--", "same.txt"]);
            git(root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("same.txt"), "staged\n").unwrap();
            git(root, &["add", "--", "same.txt"]);
        }
        Self {
            base,
            root_a,
            root_b,
        }
    }
}

impl Drop for TwoRootProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.base);
    }
}

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

fn subject(root: &Path) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["log", "-1", "--format=%s"])
        .output()
        .unwrap();
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap()
}

fn write_executable(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    }
}

#[test]
fn commits_only_the_requested_root_and_returns_all_authoritative_statuses() {
    let fixture = TwoRootProject::new();
    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    database
        .add_project_root(&project.id, &fixture.root_b)
        .unwrap();
    let roots = database.root_records(&project.id).unwrap();
    let root_a_id = roots[0].id.clone();
    let root_b_id = roots[1].id.clone();
    let state = AppState::new(database);

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id.clone(),
                root_id: root_a_id.clone(),
                message: "root A only".into(),
            },
            0,
        ))
        .unwrap();

    assert_eq!(subject(&fixture.root_a), "root A only\n");
    assert_eq!(subject(&fixture.root_b), "fixture\n");
    assert_eq!(result.project_id, project.id);
    assert_eq!(result.root_id, root_a_id);
    assert_eq!(result.transition_epoch, 0);
    assert!(result.error.is_none());
    assert_eq!(result.repositories.roots.len(), 2);
    assert!(result.repositories.roots[0].changes.is_empty());
    assert_eq!(result.repositories.roots[1].root_id, root_b_id);
    assert_eq!(
        result.repositories.roots[1].changes[0].index_status,
        "modified"
    );
}

#[test]
fn commit_and_post_commit_projection_never_launch_content_filters() {
    let fixture = TwoRootProject::new();
    fs::write(
        fixture.root_a.join(".gitattributes"),
        "*.txt filter=sentinel\n",
    )
    .unwrap();
    git(&fixture.root_a, &["add", "--", ".gitattributes"]);
    fs::write(fixture.root_a.join("same.txt"), "unstaged\n").unwrap();
    let sentinel = fixture.root_a.join("filter-ran");
    let filter = fixture.root_a.join("sentinel-filter");
    let sentinel_path = sentinel.to_string_lossy().replace('\\', "/");
    write_executable(
        &filter,
        &format!("#!/bin/sh\necho ran > \"{sentinel_path}\"\n/usr/bin/cat\n"),
    );
    let filter_command = format!("\"{}\"", filter.to_string_lossy().replace('\\', "/"));
    git(
        &fixture.root_a,
        &["config", "filter.sentinel.clean", &filter_command],
    );
    git(
        &fixture.root_a,
        &["config", "filter.sentinel.required", "true"],
    );

    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    database
        .add_project_root(&project.id, &fixture.root_b)
        .unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id,
                message: "filter-free commit".into(),
            },
            0,
        ))
        .unwrap();

    assert!(!sentinel.exists());
    assert!(result.repositories.truncated);
    assert!(result.repositories.roots[0].truncated);
    assert!(
        result.repositories.roots[0]
            .message
            .as_deref()
            .unwrap()
            .contains("index-only")
    );
    assert!(
        !result.repositories.roots[0]
            .message
            .as_deref()
            .unwrap()
            .contains("Commit completed")
    );
}

#[test]
fn post_commit_projection_preserves_a_staged_type_change() {
    let fixture = TwoRootProject::new();
    let linked_commit = Command::new("git")
        .arg("-C")
        .arg(&fixture.root_b)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(linked_commit.status.success());
    let linked_commit = String::from_utf8(linked_commit.stdout).unwrap();
    let cache_info = format!("160000,{},same.txt", linked_commit.trim());
    git(
        &fixture.root_b,
        &["update-index", "--cacheinfo", &cache_info],
    );

    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    database
        .add_project_root(&project.id, &fixture.root_b)
        .unwrap();
    let roots = database.root_records(&project.id).unwrap();
    let state = AppState::new(database);
    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id: roots[0].id.clone(),
                message: "project type change".into(),
            },
            0,
        ))
        .unwrap();

    let type_change_root = result
        .repositories
        .roots
        .iter()
        .find(|root| root.root_id == roots[1].id)
        .unwrap();
    assert_eq!(type_change_root.changes.len(), 1);
    assert_eq!(type_change_root.changes[0].index_status, "typeChanged");
}

#[test]
fn effect_unknown_recovery_never_projects_a_replacement_root_identity() {
    let fixture = TwoRootProject::new();
    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    database
        .add_project_root(&project.id, &fixture.root_b)
        .unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);
    let root = fixture.root_a.clone();
    let moved = fixture.base.join("original-repo-a");
    let hook_root = root.clone();
    let hook_moved = moved.clone();
    let policy = repository::GitProcessPolicy::mutation(Duration::from_secs(3))
        .with_after_process_hook(move || {
            fs::rename(&hook_root, &hook_moved).unwrap();
            fs::create_dir(&hook_root).unwrap();
            git(&hook_root, &["init", "-q"]);
            git(
                &hook_root,
                &["config", "user.email", "replacement@example.invalid"],
            );
            git(&hook_root, &["config", "user.name", "Replacement"]);
            fs::write(hook_root.join("replacement.txt"), "base\n").unwrap();
            git(&hook_root, &["add", "--", "replacement.txt"]);
            git(&hook_root, &["commit", "-q", "-m", "replacement"]);
            fs::write(hook_root.join("replacement.txt"), "staged\n").unwrap();
            git(&hook_root, &["add", "--", "replacement.txt"]);
        });

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner_with_policy_override(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id: root_id.clone(),
                message: "original root only".into(),
            },
            0,
            Some(policy),
        ))
        .unwrap();

    assert_eq!(
        result.error.as_ref().unwrap().kind,
        crate::domain::GitFileMutationErrorKind::EffectUnknown
    );
    let recovered = result
        .repositories
        .roots
        .iter()
        .find(|candidate| candidate.root_id == root_id)
        .unwrap();
    assert_eq!(recovered.state, "unavailable");
    assert!(recovered.changes.is_empty());
    assert_eq!(subject(&moved), "original root only\n");
    assert_eq!(subject(&root), "replacement\n");
}

#[test]
fn ordinary_git_failure_never_exposes_an_absolute_repository_path() {
    let fixture = TwoRootProject::new();
    let native_root = std::path::absolute(&fixture.root_a).unwrap();
    assert!(native_root.is_absolute());
    let missing_object_store = native_root.join("missing-object-store");
    let alternates = fixture.root_a.join(".git/objects/info/alternates");
    fs::create_dir_all(alternates.parent().unwrap()).unwrap();
    fs::write(alternates, format!("{}\n", missing_object_store.display())).unwrap();
    let head = Command::new("git")
        .arg("-C")
        .arg(&fixture.root_a)
        .args(["rev-parse", "HEAD"])
        .output()
        .unwrap();
    assert!(head.status.success());
    let head = String::from_utf8(head.stdout).unwrap();
    let head = head.trim();
    fs::remove_file(
        fixture
            .root_a
            .join(".git/objects")
            .join(&head[..2])
            .join(&head[2..]),
    )
    .unwrap();

    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);
    let error = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id,
                message: "must fail safely".into(),
            },
            0,
        ))
        .unwrap_err();
    assert!(matches!(&error, AppError::Git(_)));
    let raw = error.to_string().replace(std::path::MAIN_SEPARATOR, "/");
    let native_root = native_root
        .to_string_lossy()
        .replace(std::path::MAIN_SEPARATOR, "/");
    assert!(
        raw.contains(&native_root),
        "fixture must produce a raw Git failure containing the native absolute repository root: {raw}"
    );

    let public = commit_command_error(error);
    let message = match public {
        crate::domain::GitFileMutationCommandError::Failed { message } => message,
        _ => panic!("ordinary Git failure must remain a failed command"),
    };
    assert!(message.len() <= 512);
    assert!(
        !message
            .replace(std::path::MAIN_SEPARATOR, "/")
            .contains(&native_root)
    );
    assert!(!message.contains(".git"));
}

#[test]
fn post_commit_index_projection_never_exposes_oversized_absolute_git_diagnostics() {
    let fixture = TwoRootProject::new();
    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);
    let hook_root = fixture.root_a.clone();
    let policy = repository::GitProcessPolicy::mutation(Duration::from_secs(3))
        .with_after_process_hook(move || {
            let head = Command::new("git")
                .arg("-C")
                .arg(&hook_root)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            assert!(head.status.success());
            let head = String::from_utf8(head.stdout).unwrap();
            let alternates = hook_root.join(".git/objects/info/alternates");
            fs::create_dir_all(alternates.parent().unwrap()).unwrap();
            let missing = hook_root
                .join(format!("missing-{}", "x".repeat(70 * 1024)))
                .to_string_lossy()
                .into_owned();
            fs::write(alternates, format!("{missing}\n")).unwrap();
            let head = head.trim();
            fs::remove_file(
                hook_root
                    .join(".git/objects")
                    .join(&head[..2])
                    .join(&head[2..]),
            )
            .unwrap();
        });

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner_with_policy_override(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id: root_id.clone(),
                message: "projection diagnostic must stay native".into(),
            },
            0,
            Some(policy),
        ))
        .unwrap();

    let raw = Command::new("git")
        .arg("-C")
        .arg(&fixture.root_a)
        .args(["diff", "--cached", "--name-status", "HEAD", "--"])
        .output()
        .unwrap();
    let native_root = fixture.root_a.to_string_lossy().replace('\\', "/");
    let raw_stderr = String::from_utf8_lossy(&raw.stderr).replace('\\', "/");
    assert!(!raw.status.success());
    assert!(raw.stderr.len() > 512);
    assert!(raw_stderr.contains(&native_root));

    let projection = result
        .repositories
        .roots
        .iter()
        .find(|candidate| candidate.root_id == root_id)
        .unwrap();
    assert_eq!(projection.state, "unavailable");
    assert!(projection.changes.is_empty());
    let message = projection.message.as_deref().unwrap();
    assert!(message.len() <= 512);
    assert!(!message.replace('\\', "/").contains(&native_root));
    assert!(!message.contains(".git"));
}

#[test]
fn stale_epoch_foreign_root_and_oversized_ids_reject_before_commit() {
    let fixture = TwoRootProject::new();
    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);
    let runtime = tokio::runtime::Runtime::new().unwrap();
    *runtime.block_on(state.project_transitions.lock()) = 1;
    let request = GitRepositoryCommitRequest {
        project_id: project.id,
        root_id,
        message: "must not commit".into(),
    };

    let stale = runtime
        .block_on(commit_git_repository_inner(&state, request.clone(), 0))
        .unwrap_err();
    assert!(stale.to_string().contains("Project changed"));
    let foreign = runtime
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                root_id: "foreign-root".into(),
                ..request.clone()
            },
            1,
        ))
        .unwrap_err();
    assert!(foreign.to_string().contains("does not belong"));

    let exact = runtime
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                root_id: "r".repeat(512),
                ..request.clone()
            },
            1,
        ))
        .unwrap_err();
    assert!(exact.to_string().contains("does not belong"));
    let plus_one = runtime
        .block_on(commit_git_repository_inner(
            &state,
            GitRepositoryCommitRequest {
                root_id: "r".repeat(513),
                ..request
            },
            1,
        ))
        .unwrap_err();
    assert!(plus_one.to_string().contains("bounded contract"));
    assert_eq!(subject(&fixture.root_a), "fixture\n");
}

#[test]
fn timeout_returns_effect_unknown_with_an_authoritative_all_root_projection() {
    let fixture = TwoRootProject::new();
    let database = Database::open(&fixture.base.join("sage.db")).unwrap();
    let project = database.create_project_from_root(&fixture.root_a).unwrap();
    database
        .add_project_root(&project.id, &fixture.root_b)
        .unwrap();
    let root_id = database.root_records(&project.id).unwrap()[0].id.clone();
    let state = AppState::new(database);

    let result = tokio::runtime::Runtime::new()
        .unwrap()
        .block_on(commit_git_repository_inner_with_timeout(
            &state,
            GitRepositoryCommitRequest {
                project_id: project.id,
                root_id: root_id.clone(),
                message: "uncertain commit".into(),
            },
            0,
            Duration::ZERO,
        ))
        .unwrap();

    assert_eq!(result.root_id, root_id);
    assert_eq!(
        result.error.as_ref().unwrap().kind,
        crate::domain::GitFileMutationErrorKind::EffectUnknown
    );
    assert_eq!(result.repositories.roots.len(), 2);
    assert!(matches!(
        subject(&fixture.root_a).as_str(),
        "fixture\n" | "uncertain commit\n"
    ));
    assert_eq!(subject(&fixture.root_b), "fixture\n");
}

#[test]
fn containment_command_error_is_typed_and_has_no_repository_projection() {
    let encoded = serde_json::to_value(commit_command_error(AppError::GitContainment {
        state: GitContainmentState::PotentiallyRunning,
        message: "Git process may still be running".into(),
    }))
    .unwrap();

    assert_eq!(
        encoded,
        serde_json::json!({
            "kind": "containment",
            "processState": "potentiallyRunning",
            "message": "A prior Git process has not finished exiting. Wait for Source Control recovery before retrying."
        })
    );
    assert!(encoded.get("repositories").is_none());
}
