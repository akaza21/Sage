use crate::{
    domain::{GitFileMutationAction, ResourceRef},
    error::{AppError, AppResult},
    git_budget, repository,
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
    mutate_file_with_timeout(workspace, resource, action, git_budget::MUTATION).await
}

pub(crate) async fn mutate_file_with_timeout(
    workspace: &Workspace,
    resource: &ResourceRef,
    action: GitFileMutationAction,
    mutation_timeout: Duration,
) -> AppResult<()> {
    mutate_file_with_process_policy(
        workspace,
        resource,
        action,
        repository::GitProcessPolicy::mutation(mutation_timeout),
    )
    .await
}

pub(crate) async fn mutate_file_with_process_policy(
    workspace: &Workspace,
    resource: &ResourceRef,
    action: GitFileMutationAction,
    process_policy: repository::GitProcessPolicy,
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
    if matches!(
        action,
        GitFileMutationAction::KeepOurs | GitFileMutationAction::KeepTheirs
    ) {
        return crate::repository_conflict::resolve_conflicted_file(
            workspace,
            resource,
            change,
            action,
            process_policy,
        )
        .await;
    }
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
            repository::run_git_with_policy(
                workspace.root(),
                &arguments,
                MAX_GIT_MUTATION_OUTPUT_BYTES,
                process_policy,
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
            repository::run_git_with_policy(
                workspace.root(),
                &arguments,
                MAX_GIT_MUTATION_OUTPUT_BYTES,
                process_policy,
            )
            .await?
        }
        GitFileMutationAction::Discard => {
            if !matches!(
                change.worktree_status.as_str(),
                "modified" | "typeChanged" | "deleted"
            ) || change.index_status == "renamed"
                || change.index_status == "copied"
                || change.original_resource.is_some()
            {
                return Err(AppError::InvalidRequest(
                    "resource worktree change cannot be safely discarded".into(),
                ));
            }
            repository::run_git_with_policy(
                workspace.root(),
                &[
                    "restore",
                    "--worktree",
                    "--",
                    resource.relative_path.as_str(),
                ],
                MAX_GIT_MUTATION_OUTPUT_BYTES,
                process_policy,
            )
            .await?
        }
        GitFileMutationAction::KeepOurs | GitFileMutationAction::KeepTheirs => {
            return Err(AppError::InvalidRequest(
                "conflicted files require the later conflict workflow".into(),
            ));
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
    let canonical = workspace.git_resource_identity(resource)?;
    if canonical != *resource {
        return Err(AppError::InvalidRequest(
            "Git mutation requires the canonical root-qualified resource identity".into(),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{ffi::OsStr, fs, path::Path, process::Command};

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

    fn create_file_symlink(target: &Path, link: &Path) -> bool {
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(target, link).unwrap();
            true
        }
        #[cfg(windows)]
        {
            match std::os::windows::fs::symlink_file(target, link) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("optional Windows Git symlink test unavailable: {error}");
                    false
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, link);
            false
        }
    }

    fn ambient_fixture(root: &Path, environment: &[(&str, &OsStr)]) -> std::process::Output {
        let mut command = Command::new(std::env::current_exe().unwrap());
        command
            .args([
                "--exact",
                "repository_mutation::tests::ambient_git_mutation_fixture",
                "--nocapture",
            ])
            .env("SAGE_GIT_AMBIENT_FIXTURE_ROOT", root);
        for (name, value) in environment {
            command.env(name, value);
        }
        command.output().unwrap()
    }

    #[test]
    fn ambient_git_mutation_fixture() {
        let Some(root) = std::env::var_os("SAGE_GIT_AMBIENT_FIXTURE_ROOT") else {
            return;
        };
        let root = Path::new(&root);
        let workspace = Workspace::open("project-a", "root-a", root).unwrap();
        let resource = workspace.resource("target.txt").unwrap();
        tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
    }

    #[test]
    fn ambient_external_index_cannot_redirect_a_git_mutation() {
        let base = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        let root = base.join("repo");
        let outside_index = base.join("outside.index");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("target.txt"), "actual\n").unwrap();

        let output = ambient_fixture(&root, &[("GIT_INDEX_FILE", outside_index.as_os_str())]);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let projected = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(repository::status(&workspace))
            .unwrap();

        assert_eq!(projected.changes[0].index_status, "added");
        assert_eq!(projected.changes[0].worktree_status, "unchanged");
        assert!(!outside_index.exists());
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn ambient_external_config_cannot_redirect_index_content() {
        let base = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        let root = base.join("repo");
        let attributes = base.join("external.attributes");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("target.txt"), "actual\n").unwrap();
        fs::write(&attributes, "target.txt filter=redirect\n").unwrap();
        let attributes_string = attributes.to_string_lossy().into_owned();

        let output = ambient_fixture(
            &root,
            &[
                ("GIT_CONFIG_COUNT", OsStr::new("2")),
                ("GIT_CONFIG_KEY_0", OsStr::new("core.attributesfile")),
                ("GIT_CONFIG_VALUE_0", OsStr::new(&attributes_string)),
                ("GIT_CONFIG_KEY_1", OsStr::new("filter.redirect.clean")),
                ("GIT_CONFIG_VALUE_1", OsStr::new("git hash-object --stdin")),
            ],
        );
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let indexed = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["show", ":target.txt"])
            .output()
            .unwrap();

        assert!(indexed.status.success());
        assert_eq!(indexed.stdout, b"actual\n");
        fs::remove_dir_all(base).unwrap();
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
    fn stage_and_unstage_treat_a_glob_filename_as_one_literal_resource() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        let target = "literal[ab].txt";
        let decoy = "literala.txt";
        fs::write(root.join(target), "target base\n").unwrap();
        fs::write(root.join(decoy), "decoy base\n").unwrap();
        git(&root, &["add", "-A", "--"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join(target), "target changed\n").unwrap();
        fs::write(root.join(decoy), "decoy changed\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource(target).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
        let staged = runtime.block_on(repository::status(&workspace)).unwrap();
        let target_change = staged
            .changes
            .iter()
            .find(|change| change.resource.relative_path == target)
            .unwrap();
        let decoy_change = staged
            .changes
            .iter()
            .find(|change| change.resource.relative_path == decoy)
            .unwrap();
        assert_eq!(target_change.index_status, "modified");
        assert_eq!(target_change.worktree_status, "unchanged");
        assert_eq!(decoy_change.index_status, "unchanged");
        assert_eq!(decoy_change.worktree_status, "modified");

        git(&root, &["add", "--", decoy]);
        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Unstage,
            ))
            .unwrap();
        let unstaged = runtime.block_on(repository::status(&workspace)).unwrap();
        let target_change = unstaged
            .changes
            .iter()
            .find(|change| change.resource.relative_path == target)
            .unwrap();
        let decoy_change = unstaged
            .changes
            .iter()
            .find(|change| change.resource.relative_path == decoy)
            .unwrap();
        assert_eq!(target_change.index_status, "unchanged");
        assert_eq!(target_change.worktree_status, "modified");
        assert_eq!(decoy_change.index_status, "modified");
        assert_eq!(decoy_change.worktree_status, "unchanged");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stages_real_tracked_modification_and_deletion_changes() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("modified.txt"), "base\n").unwrap();
        fs::write(root.join("deleted.txt"), "delete me\n").unwrap();
        git(&root, &["add", "-A", "--"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("modified.txt"), "changed\n").unwrap();
        fs::remove_file(root.join("deleted.txt")).unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let modified = workspace.resource("modified.txt").unwrap();
        let deleted = ResourceRef {
            root_id: "root-a".into(),
            relative_path: "deleted.txt".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &modified,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
        runtime
            .block_on(mutate_file(
                &workspace,
                &deleted,
                GitFileMutationAction::Stage,
            ))
            .unwrap();
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();
        let modified = projected
            .changes
            .iter()
            .find(|change| change.resource.relative_path == "modified.txt")
            .unwrap();
        let deleted = projected
            .changes
            .iter()
            .find(|change| change.resource.relative_path == "deleted.txt")
            .unwrap();

        assert_eq!(modified.index_status, "modified");
        assert_eq!(modified.worktree_status, "unchanged");
        assert_eq!(deleted.index_status, "deleted");
        assert_eq!(deleted.worktree_status, "unchanged");
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
    fn discards_one_literal_modified_file_and_restores_a_tracked_deletion() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        let target = "literal[ab].txt";
        let decoy = "literala.txt";
        fs::create_dir_all(root.join("nested")).unwrap();
        fs::write(root.join(target), "target base\n").unwrap();
        fs::write(root.join(decoy), "decoy base\n").unwrap();
        fs::write(root.join("nested/deleted.txt"), "restore me\n").unwrap();
        git(&root, &["add", "-A", "--"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join(target), "target changed\n").unwrap();
        fs::write(root.join(decoy), "decoy changed\n").unwrap();
        fs::remove_dir_all(root.join("nested")).unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let target_resource = workspace.resource(target).unwrap();
        let deleted_resource = ResourceRef {
            root_id: "root-a".into(),
            relative_path: "nested/deleted.txt".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &target_resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap();
        runtime
            .block_on(mutate_file(
                &workspace,
                &deleted_resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap();

        assert_eq!(
            fs::read_to_string(root.join(target)).unwrap(),
            "target base\n"
        );
        assert_eq!(
            fs::read_to_string(root.join(decoy)).unwrap(),
            "decoy changed\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("nested/deleted.txt")).unwrap(),
            "restore me\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_preserves_the_index_for_partially_staged_and_added_files() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
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
        fs::write(root.join("first.txt"), "staged first\n").unwrap();
        git(&root, &["add", "--", "first.txt"]);
        fs::write(root.join("first.txt"), "unstaged first\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for path in ["partial.txt", "first.txt"] {
            runtime
                .block_on(mutate_file(
                    &workspace,
                    &workspace.resource(path).unwrap(),
                    GitFileMutationAction::Discard,
                ))
                .unwrap();
        }

        assert_eq!(
            fs::read_to_string(root.join("partial.txt")).unwrap(),
            "staged\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("first.txt")).unwrap(),
            "staged first\n"
        );
        let partial_index = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["show", ":partial.txt"])
            .output()
            .unwrap();
        let first_index = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["show", ":first.txt"])
            .output()
            .unwrap();
        assert_eq!(partial_index.stdout, b"staged\n");
        assert_eq!(first_index.stdout, b"staged first\n");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_restores_an_unborn_staged_file_from_the_index() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("first.txt"), "staged first\n").unwrap();
        git(&root, &["add", "--", "first.txt"]);
        fs::write(root.join("first.txt"), "unstaged first\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("first.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap();

        assert_eq!(
            fs::read_to_string(root.join("first.txt")).unwrap(),
            "staged first\n"
        );
        let projected = runtime.block_on(repository::status(&workspace)).unwrap();
        assert_eq!(projected.changes[0].index_status, "added");
        assert_eq!(projected.changes[0].worktree_status, "unchanged");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_of_a_retargeted_tracked_symlink_never_restores_its_modified_target() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.symlinks", "true"]);
        fs::write(root.join("target-a.txt"), "target a\n").unwrap();
        fs::write(root.join("target-b.txt"), "target b\n").unwrap();
        if !create_file_symlink(Path::new("target-a.txt"), &root.join("link.txt")) {
            fs::remove_dir_all(root).unwrap();
            return;
        }
        git(&root, &["add", "-A", "--"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("target-b.txt"), "target b changed\n").unwrap();
        fs::remove_file(root.join("link.txt")).unwrap();
        create_file_symlink(Path::new("target-b.txt"), &root.join("link.txt"));
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = ResourceRef {
            root_id: "root-a".into(),
            relative_path: "link.txt".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let before = runtime.block_on(repository::status(&workspace)).unwrap();
        let link_change = before
            .changes
            .iter()
            .find(|change| change.resource.relative_path == "link.txt")
            .unwrap();
        assert_eq!(link_change.worktree_status, "modified");

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap();

        assert_eq!(
            fs::read_link(root.join("link.txt")).unwrap(),
            Path::new("target-a.txt")
        );
        assert_eq!(
            fs::read_to_string(root.join("target-b.txt")).unwrap(),
            "target b changed\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn discard_type_change_replaces_an_outside_symlink_without_touching_its_target() {
        let base = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        let root = base.join("repo");
        let outside = base.join("outside.txt");
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.symlinks", "true"]);
        fs::write(root.join("link.txt"), "tracked base\n").unwrap();
        fs::write(&outside, "outside unchanged\n").unwrap();
        git(&root, &["add", "--", "link.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::remove_file(root.join("link.txt")).unwrap();
        if !create_file_symlink(&outside, &root.join("link.txt")) {
            fs::remove_dir_all(base).unwrap();
            return;
        }
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = ResourceRef {
            root_id: "root-a".into(),
            relative_path: "link.txt".into(),
        };
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let before = runtime.block_on(repository::status(&workspace)).unwrap();
        let link_change = before
            .changes
            .iter()
            .find(|change| change.resource == resource)
            .unwrap();
        assert_eq!(link_change.worktree_status, "typeChanged");

        runtime
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap();

        assert!(
            !fs::symlink_metadata(root.join("link.txt"))
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(
            fs::read_to_string(root.join("link.txt")).unwrap(),
            "tracked base\n"
        );
        assert_eq!(fs::read_to_string(outside).unwrap(), "outside unchanged\n");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn discard_rejects_untracked_staged_only_and_renamed_files_without_deleting_bytes() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        fs::write(root.join("rename-before.txt"), "rename\n").unwrap();
        git(&root, &["add", "-A", "--"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("untracked.txt"), "keep untracked\n").unwrap();
        fs::write(root.join("tracked.txt"), "staged only\n").unwrap();
        git(&root, &["add", "--", "tracked.txt"]);
        fs::rename(
            root.join("rename-before.txt"),
            root.join("rename-after.txt"),
        )
        .unwrap();
        git(&root, &["add", "-A", "--"]);
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        for path in ["untracked.txt", "tracked.txt", "rename-after.txt"] {
            let resource = workspace.resource(path).unwrap();
            let error = runtime
                .block_on(mutate_file(
                    &workspace,
                    &resource,
                    GitFileMutationAction::Discard,
                ))
                .unwrap_err();
            assert!(error.to_string().contains("cannot be safely discarded"));
        }

        assert_eq!(
            fs::read_to_string(root.join("untracked.txt")).unwrap(),
            "keep untracked\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("tracked.txt")).unwrap(),
            "staged only\n"
        );
        assert_eq!(
            fs::read_to_string(root.join("rename-after.txt")).unwrap(),
            "rename\n"
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
    fn discard_same_relative_path_changes_only_the_selected_root() {
        let base = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        let root_a = base.join("a");
        let root_b = base.join("b");
        for (root, content) in [(&root_a, "base a\n"), (&root_b, "base b\n")] {
            fs::create_dir_all(root).unwrap();
            git(root, &["init", "-q"]);
            git(root, &["config", "user.email", "sage@example.invalid"]);
            git(root, &["config", "user.name", "Sage Test"]);
            fs::write(root.join("same.txt"), content).unwrap();
            git(root, &["add", "--", "same.txt"]);
            git(root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("same.txt"), "changed\n").unwrap();
        }
        let workspace_a = Workspace::open("project-a", "root-a", &root_a).unwrap();
        let resource_a = workspace_a.resource("same.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        runtime
            .block_on(mutate_file(
                &workspace_a,
                &resource_a,
                GitFileMutationAction::Discard,
            ))
            .unwrap();

        assert_eq!(
            fs::read_to_string(root_a.join("same.txt")).unwrap(),
            "base a\n"
        );
        assert_eq!(
            fs::read_to_string(root_b.join("same.txt")).unwrap(),
            "changed\n"
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn discard_rejects_an_unmerged_file_without_changing_conflict_bytes() {
        let root = std::env::temp_dir().join(format!("sage-git-discard-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("conflict.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "conflict.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "other"]);
        fs::write(root.join("conflict.txt"), "other\n").unwrap();
        git(&root, &["commit", "-q", "-am", "other"]);
        git(&root, &["checkout", "-q", "-"]);
        fs::write(root.join("conflict.txt"), "master\n").unwrap();
        git(&root, &["commit", "-q", "-am", "master"]);
        let merge = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["merge", "other"])
            .output()
            .unwrap();
        assert!(!merge.status.success());
        let before = fs::read(root.join("conflict.txt")).unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("conflict.txt").unwrap();

        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(mutate_file(
                &workspace,
                &resource,
                GitFileMutationAction::Discard,
            ))
            .unwrap_err();

        assert!(error.to_string().contains("later conflict workflow"));
        assert_eq!(fs::read(root.join("conflict.txt")).unwrap(), before);
        fs::remove_dir_all(root).unwrap();
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
    fn a_post_spawn_git_deadline_reaps_before_return_and_never_mutates_later() {
        let root = std::env::temp_dir().join(format!("sage-git-mutate-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        fs::write(root.join("timeout.txt"), "timeout\n").unwrap();
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let resource = workspace.resource("timeout.txt").unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();

        let error = runtime
            .block_on(mutate_file_with_process_policy(
                &workspace,
                &resource,
                GitFileMutationAction::Stage,
                repository::GitProcessPolicy::post_spawn_failure(Duration::from_secs(3)),
            ))
            .unwrap_err();

        // The failure is injected after spawn, so Git may or may not have
        // staged the file before it was killed: that is why the effect is
        // unknown. Either way the process is reaped before the error returns,
        // so nothing may change afterwards.
        assert!(matches!(error, AppError::GitEffectUnknown(_)));
        let immediate = runtime.block_on(repository::status(&workspace)).unwrap();
        let observed = (
            immediate.changes[0].index_status.clone(),
            immediate.changes[0].worktree_status.clone(),
        );
        assert!(
            matches!(
                (observed.0.as_str(), observed.1.as_str()),
                ("unchanged", "untracked") | ("added", "unchanged")
            ),
            "unexpected state after the injected failure: {observed:?}"
        );
        std::thread::sleep(Duration::from_millis(200));
        let later = runtime.block_on(repository::status(&workspace)).unwrap();
        assert_eq!(
            (
                later.changes[0].index_status.as_str(),
                later.changes[0].worktree_status.as_str(),
            ),
            (observed.0.as_str(), observed.1.as_str())
        );
        fs::remove_dir_all(root).unwrap();
    }
}
