use crate::{
    error::{AppError, AppResult},
    git_budget, repository,
    workspace::Workspace,
};

const MAX_GIT_ABORT_OUTPUT_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ExclusiveSequence {
    Merge,
    Rebase,
    CherryPick,
    Revert,
}

pub(crate) fn admit_exclusive_sequence(
    merge: bool,
    rebase: bool,
    cherry_pick: bool,
    revert: bool,
) -> AppResult<ExclusiveSequence> {
    let kinds = [
        merge.then_some(ExclusiveSequence::Merge),
        rebase.then_some(ExclusiveSequence::Rebase),
        cherry_pick.then_some(ExclusiveSequence::CherryPick),
        revert.then_some(ExclusiveSequence::Revert),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>();
    match kinds.as_slice() {
        [kind] => Ok(*kind),
        [] => Err(AppError::InvalidRequest(
            "Git abort requires an in-progress merge, rebase, cherry-pick, or revert".into(),
        )),
        _ => Err(AppError::InvalidRequest(
            "Git abort requires exactly one in-progress merge, rebase, cherry-pick, or revert"
                .into(),
        )),
    }
}

fn abort_args(kind: ExclusiveSequence) -> &'static [&'static str] {
    match kind {
        ExclusiveSequence::Merge => &["merge", "--abort"],
        ExclusiveSequence::Rebase => &["rebase", "--abort"],
        ExclusiveSequence::CherryPick => &["cherry-pick", "--abort"],
        ExclusiveSequence::Revert => &["revert", "--abort"],
    }
}

pub(crate) async fn abort_in_progress(workspace: &Workspace) -> AppResult<()> {
    abort_in_progress_with_policy(
        workspace,
        repository::GitProcessPolicy::mutation(git_budget::MUTATION),
    )
    .await
}

pub(crate) async fn abort_in_progress_with_policy(
    workspace: &Workspace,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        repository::RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git abort requires a Project root that equals the repository top level".into(),
        ));
    }
    let kind = admit_exclusive_sequence(
        crate::repository_conflict::merge_in_progress(workspace).await?,
        crate::repository_conflict::rebase_in_progress(workspace).await?,
        crate::repository_conflict::cherry_pick_in_progress(workspace).await?,
        crate::repository_conflict::revert_in_progress(workspace).await?,
    )?;
    let output = repository::run_git_with_policy(
        workspace.root(),
        abort_args(kind),
        MAX_GIT_ABORT_OUTPUT_BYTES,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Workspace;
    use std::{fs, path::Path, process::Command};
    use uuid::Uuid;

    #[test]
    fn admits_exactly_one_in_progress_sequence() {
        assert_eq!(
            admit_exclusive_sequence(true, false, false, false).unwrap(),
            ExclusiveSequence::Merge
        );
        assert_eq!(
            admit_exclusive_sequence(false, true, false, false).unwrap(),
            ExclusiveSequence::Rebase
        );
        assert_eq!(
            admit_exclusive_sequence(false, false, true, false).unwrap(),
            ExclusiveSequence::CherryPick
        );
        assert_eq!(
            admit_exclusive_sequence(false, false, false, true).unwrap(),
            ExclusiveSequence::Revert
        );
        assert!(admit_exclusive_sequence(false, false, false, false).is_err());
        assert!(admit_exclusive_sequence(true, true, false, false).is_err());
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

    fn merge_conflict_fixture() -> (std::path::PathBuf, Workspace) {
        let root = std::env::temp_dir().join(format!("sage-git-abort-merge-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("conflict.txt"), "base\n").unwrap();
        git(&root, &["add", "--", "conflict.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["checkout", "-q", "-b", "other"]);
        fs::write(root.join("conflict.txt"), "other\n").unwrap();
        git(&root, &["commit", "-q", "-am", "other"]);
        git(&root, &["checkout", "-q", "-"]);
        fs::write(root.join("conflict.txt"), "ours\n").unwrap();
        git(&root, &["commit", "-q", "-am", "ours"]);
        let merge = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["merge", "other"])
            .output()
            .unwrap();
        assert!(!merge.status.success());
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    fn current_branch(root: &Path) -> String {
        let output = Command::new("git")
            .arg("-C")
            .arg(root)
            .args(["branch", "--show-current"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    fn rebase_conflict_fixture() -> (std::path::PathBuf, Workspace) {
        let (root, workspace) = merge_conflict_fixture();
        git(&root, &["merge", "--abort"]);
        let onto = current_branch(&root);
        git(&root, &["checkout", "-q", "other"]);
        let rebase = Command::new("git")
            .arg("-C")
            .arg(&root)
            .args(["rebase", &onto])
            .output()
            .unwrap();
        assert!(!rebase.status.success());
        (root, workspace)
    }

    fn idle_fixture() -> (std::path::PathBuf, Workspace) {
        let root = std::env::temp_dir().join(format!("sage-git-abort-idle-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("ok.txt"), "ok\n").unwrap();
        git(&root, &["add", "--", "ok.txt"]);
        git(&root, &["commit", "-q", "-m", "ok"]);
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        (root, workspace)
    }

    #[test]
    fn aborts_a_conflicted_merge_without_completing_it() {
        let (root, workspace) = merge_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(abort_in_progress(&workspace)).unwrap();
        assert!(
            !runtime
                .block_on(crate::repository_conflict::merge_in_progress(&workspace))
                .unwrap()
        );
        assert_eq!(
            fs::read_to_string(root.join("conflict.txt")).unwrap(),
            "ours\n"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn aborts_a_conflicted_rebase_without_continuing_it() {
        let (root, workspace) = rebase_conflict_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        runtime.block_on(abort_in_progress(&workspace)).unwrap();
        assert!(
            !runtime
                .block_on(crate::repository_conflict::rebase_in_progress(&workspace))
                .unwrap()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn live_abort_rejects_an_idle_repository() {
        let (root, workspace) = idle_fixture();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime.block_on(abort_in_progress(&workspace));
        fs::remove_dir_all(root).unwrap();
        assert!(error.is_err());
    }
}
