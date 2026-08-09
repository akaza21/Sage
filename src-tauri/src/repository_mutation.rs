use crate::{
    domain::{GitFileMutationAction, ResourceRef},
    error::{AppError, AppResult},
    repository,
    workspace::Workspace,
};
use std::time::Duration;

const MAX_GIT_ROOT_ID_BYTES: usize = 512;
const MAX_GIT_RESOURCE_PATH_BYTES: usize = 4 * 1024;
const MAX_GIT_MUTATION_OUTPUT_BYTES: usize = 64 * 1024;

pub(crate) async fn mutate_file(
    workspace: &Workspace,
    resource: &ResourceRef,
    action: GitFileMutationAction,
) -> AppResult<()> {
    mutate_file_with_timeout(workspace, resource, action, Duration::from_secs(3)).await
}

async fn mutate_file_with_timeout(
    workspace: &Workspace,
    resource: &ResourceRef,
    action: GitFileMutationAction,
    mutation_timeout: Duration,
) -> AppResult<()> {
    validate_resource_identity(workspace, resource)?;
    let projected = repository::status(workspace).await?;
    if projected.state != "repository" {
        return Err(AppError::InvalidRequest(
            "Git mutations require a Project root that equals the repository top level".into(),
        ));
    }
    let change = projected
        .changes
        .iter()
        .find(|change| change.resource == *resource)
        .ok_or_else(|| AppError::InvalidRequest("resource is not a current Git change".into()))?;
    if change.index_status == "conflicted" || change.worktree_status == "conflicted" {
        return Err(AppError::InvalidRequest(
            "conflicted files require the later conflict workflow".into(),
        ));
    }
    let mut paths = vec![resource.relative_path.clone()];
    if let Some(original) = &change.original_resource {
        validate_resource_identity(workspace, original)?;
        paths.push(original.relative_path.clone());
    }
    let output = match action {
        GitFileMutationAction::Stage => {
            if change.worktree_status == "unchanged" || change.worktree_status == "unknown" {
                return Err(AppError::InvalidRequest(
                    "resource has no worktree change to stage".into(),
                ));
            }
            let mut arguments = vec!["add", "--"];
            arguments.extend(paths.iter().map(String::as_str));
            repository::run_git_with_timeout(
                workspace.root(),
                &arguments,
                MAX_GIT_MUTATION_OUTPUT_BYTES,
                mutation_timeout,
            )
            .await?
        }
        GitFileMutationAction::Unstage => {
            if change.index_status == "unchanged" || change.index_status == "unknown" {
                return Err(AppError::InvalidRequest(
                    "resource has no index change to unstage".into(),
                ));
            }
            let head = repository::run_git(
                workspace.root(),
                &["rev-parse", "--verify", "--quiet", "HEAD"],
                256,
            )
            .await?;
            if !head.succeeded() && head.exit_code() != Some(1) {
                return repository::ensure_success(&head);
            }
            let mut arguments = if head.succeeded() {
                vec!["restore", "--staged", "--"]
            } else {
                vec!["rm", "--cached", "-f", "--"]
            };
            arguments.extend(paths.iter().map(String::as_str));
            repository::run_git_with_timeout(
                workspace.root(),
                &arguments,
                MAX_GIT_MUTATION_OUTPUT_BYTES,
                mutation_timeout,
            )
            .await?
        }
    };
    repository::ensure_success(&output)
}

fn validate_resource_identity(workspace: &Workspace, resource: &ResourceRef) -> AppResult<()> {
    let root_id_bytes = resource.root_id.len();
    let path_bytes = resource.relative_path.len();
    if root_id_bytes == 0
        || root_id_bytes > MAX_GIT_ROOT_ID_BYTES
        || path_bytes == 0
        || path_bytes > MAX_GIT_RESOURCE_PATH_BYTES
    {
        return Err(AppError::InvalidRequest(
            "Git mutation resource identity exceeds its bounded contract".into(),
        ));
    }
    let canonical = workspace.canonical_ingress_resource(resource)?;
    if canonical != *resource {
        return Err(AppError::InvalidRequest(
            "Git mutation requires the canonical root-qualified resource identity".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path, process::Command};

    use uuid::Uuid;

    use super::*;
    use crate::{domain::GitFileMutationAction, repository, workspace::Workspace};

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
    fn stages_an_untracked_weird_filename_as_one_literal_resource() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        let path = "-- odd [name] ü.txt";
        fs::write(root.join(path), "untracked\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource(path).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();

        assert_eq!(projected.changes.len(), 1);
        assert_eq!(projected.changes[0].resource, resource);
        assert_eq!(projected.changes[0].index_status, "added");
        assert_eq!(projected.changes[0].worktree_status, "unchanged");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unstages_a_partially_staged_file_without_changing_the_worktree() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("partial.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "partial.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("partial.txt"), "staged\n").unwrap();
        git(&root, &["add", "--", "partial.txt"]);
        fs::write(root.join("partial.txt"), "unstaged\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("partial.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Unstage,
            ))
            .unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();

        assert_eq!(
            fs::read_to_string(root.join("partial.txt")).unwrap(),
            "unstaged\n"
        );
        assert_eq!(projected.changes.len(), 1);
        assert_eq!(projected.changes[0].index_status, "unchanged");
        assert_eq!(projected.changes[0].worktree_status, "modified");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unstages_both_sides_of_one_staged_rename() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("before.txt"), "renamed\n").unwrap();
        git(&root, &["add", "--", "before.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        let renamed = "-- renamed ü.txt";
        fs::rename(root.join("before.txt"), root.join(renamed)).unwrap();
        git(&root, &["add", "-A", "--"]);
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let projected = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(repository::status(&workspace))
            .unwrap();
        let change = projected
            .changes
            .iter()
            .find(|change| change.index_status == "renamed")
            .unwrap();
        let resource = change.resource.clone();
        assert_eq!(
            change.original_resource.as_ref().unwrap().relative_path,
            "before.txt"
        );
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Unstage,
            ))
            .unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();

        assert!(
            projected
                .changes
                .iter()
                .all(|change| change.index_status == "unchanged")
        );
        assert!(root.join(renamed).is_file());
        assert!(!root.join("before.txt").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unstages_a_new_file_in_an_unborn_repository() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("first.txt"), "first\n").unwrap();
        git(&root, &["add", "--", "first.txt"]);
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("first.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Unstage,
            ))
            .unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();

        assert_eq!(projected.changes.len(), 1);
        assert_eq!(projected.changes[0].index_status, "unchanged");
        assert_eq!(projected.changes[0].worktree_status, "untracked");
        assert_eq!(
            fs::read_to_string(root.join("first.txt")).unwrap(),
            "first\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn same_relative_path_mutates_only_the_selected_project_root() {
        let base = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        for root in [&root_a, &root_b] {
            fs::create_dir_all(root).unwrap();
            git(root, &["init", "-q"]);
            fs::write(root.join("same.txt"), "same\n").unwrap();
        }
        let workspace_a = Workspace::open("project-a", "root-a", &root_a).unwrap();
        let workspace_b = Workspace::open("project-a", "root-b", &root_b).unwrap();
        let resource_a = workspace_a.resource("same.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace_a,
                &resource_a,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
        let projected_a = runtime.block_on(repository::status(&workspace_a)).unwrap();
        let projected_b = runtime.block_on(repository::status(&workspace_b)).unwrap();

        assert_eq!(projected_a.changes[0].index_status, "added");
        assert_eq!(projected_b.changes[0].index_status, "unchanged");
        assert_eq!(projected_b.changes[0].worktree_status, "untracked");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn a_git_failure_preserves_the_index_and_worktree() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("blocked.txt"), "blocked\n").unwrap();
        fs::write(root.join(".git/index.lock"), "held").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("blocked.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let error = runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
            ))
            .unwrap_err();

        assert!(error.to_string().contains("index.lock"));
        assert_eq!(
            fs::read_to_string(root.join("blocked.txt")).unwrap(),
            "blocked\n"
        );
        fs::remove_file(root.join(".git/index.lock")).unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();
        assert_eq!(projected.changes[0].worktree_status, "untracked");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn a_git_deadline_expires_without_mutating_the_index() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("timeout.txt"), "timeout\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("timeout.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let error = runtime
            .block_on(mutate_file_with_timeout(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
                Duration::ZERO,
            ))
            .unwrap_err();

        assert!(error.to_string().contains("bounded deadline"));
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();
        assert_eq!(projected.changes[0].worktree_status, "untracked");
        fs::remove_dir_all(root).unwrap();
    }
}
