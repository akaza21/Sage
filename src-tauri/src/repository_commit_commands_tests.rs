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
        let base = std::env::temp_dir().join(format!("sage-git-command-{}", Uuid::new_v4()));
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
            "message": "Git process may still be running"
        })
    );
    assert!(encoded.get("repositories").is_none());
}
