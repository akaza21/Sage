pub use crate::repository_diff::file_diff;
pub(crate) use crate::repository_diff::repository_diff_base;
pub(crate) use crate::repository_git::{
    GitOutput, GitProcessPolicy, RepositoryBoundary, ensure_success, repository_boundary,
    repository_boundary_from_output, run_git, run_git_with_authority, run_git_with_policy,
};
pub(crate) use crate::repository_status::tracking_status;
pub use crate::repository_status::{project_status, status};

#[cfg(test)]
pub(crate) use crate::git_process::GitProcessTestHarness;
#[cfg(all(test, windows))]
pub(crate) use crate::repository_status::canonicalize_numstat_paths;

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use uuid::Uuid;

    use super::*;
    use crate::workspace::Workspace;

    #[cfg(windows)]
    use std::collections::HashMap;

    fn git(root: &Path, args: &[&str]) {
        assert!(
            crate::git_process::fixture_git()
                .arg("-C")
                .arg(root)
                .args(args)
                .status()
                .unwrap()
                .success()
        );
    }

    #[test]
    fn reads_status_and_diff_from_the_git_cli() {
        let root = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("src")).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("src/main.rs"), "fn old() {}\n").unwrap();
        git(&root, &["add", "src/main.rs"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("src/main.rs"), "fn new() {}\n").unwrap();

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let status = runtime.block_on(status(&workspace)).unwrap();
        let diff = runtime
            .block_on(file_diff(
                &workspace,
                &workspace.resource("src/main.rs").unwrap(),
            ))
            .unwrap();

        assert_eq!(status.changes.len(), 1);
        assert_eq!(status.changes[0].resource.relative_path, "src/main.rs");
        assert_eq!(status.changes[0].additions, Some(1));
        assert_eq!(status.changes[0].deletions, Some(1));
        assert_eq!(diff.hunks.len(), 1);
        assert_eq!(diff.hunks[0].lines[0].kind, "removed");
        assert_eq!(diff.hunks[0].lines[1].kind, "added");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn diff_view_reports_repository_textconv_refusal_without_running_the_driver() {
        let root = std::env::temp_dir().join(format!("sage-git-textconv-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join(".gitattributes"), "*.txt diff=sentinel\n").unwrap();
        fs::write(root.join("notes.txt"), "old\n").unwrap();
        git(&root, &["add", "--", ".gitattributes", "notes.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::write(root.join("notes.txt"), "new\n").unwrap();
        let sentinel = root.join("textconv-ran");
        git(
            &root,
            &[
                "config",
                "diff.sentinel.textconv",
                "echo ran > textconv-ran; cat",
            ],
        );

        // Plain Git runs the driver: the sentinel is a live execution witness.
        git(&root, &["diff", "--", "notes.txt"]);
        assert!(sentinel.exists());
        fs::remove_file(&sentinel).unwrap();

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let error = runtime
            .block_on(file_diff(
                &workspace,
                &workspace.resource("notes.txt").unwrap(),
            ))
            .unwrap_err();

        assert!(matches!(
            error,
            crate::error::AppError::UnsafeRepositoryConfig(_)
        ));
        let message = error.to_string();
        assert!(message.contains("Sage did not run Git here"), "{message}");
        assert!(message.contains("diff.sentinel.textconv"), "{message}");
        assert!(
            message.contains("Remove them from the repository config, or use the terminal"),
            "{message}"
        );
        let projection =
            runtime.block_on(project_status(vec![("root-test".into(), Ok(workspace))]));
        assert_eq!(projection.roots[0].state, "unavailable");
        assert_eq!(
            projection.roots[0].message.as_deref(),
            Some(message.as_str())
        );
        assert!(!sentinel.exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn numstat_casing_is_canonicalized_before_status_merge() {
        let root = std::env::temp_dir().join(format!("sage-git-case-{}", Uuid::new_v4()));
        fs::create_dir_all(root.join("Source")).unwrap();
        fs::write(root.join("Source/Main.rs"), "changed\n").unwrap();
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let stats = HashMap::from([("source/main.rs".into(), (Some(3), Some(2)))]);

        let canonical = canonicalize_numstat_paths(&workspace, stats).unwrap();
        assert_eq!(canonical.get("Source/Main.rs"), Some(&(Some(3), Some(2))));
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn deleted_tracked_backslash_name_is_rejected_before_resource_ingress() {
        let root = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("tracked\\name.txt"), "tracked\n").unwrap();
        git(&root, &["add", "--", "tracked\\name.txt"]);
        git(&root, &["commit", "-q", "-m", "fixture"]);
        fs::remove_file(root.join("tracked\\name.txt")).unwrap();

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let error = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(status(&workspace))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("canonical root-relative spelling")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unborn_repository_counts_the_worktree_against_an_empty_tree() {
        let root = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("new.txt"), "first\n").unwrap();
        git(&root, &["add", "new.txt"]);
        fs::write(root.join("new.txt"), "first\nsecond\n").unwrap();

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let status = runtime.block_on(status(&workspace)).unwrap();

        assert_eq!(status.changes.len(), 1);
        assert_eq!(status.changes[0].index_status, "added");
        assert_eq!(status.changes[0].worktree_status, "modified");
        assert_eq!(status.changes[0].additions, Some(2));
        assert_eq!(status.changes[0].deletions, Some(0));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn status_projects_an_exact_tracking_ref_and_ahead_count() {
        let root = std::env::temp_dir().join(format!("sage-git-tracking-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q", "-b", "main"]);
        git(&root, &["config", "user.email", "sage@example.invalid"]);
        git(&root, &["config", "user.name", "Sage Test"]);
        fs::write(root.join("tracked.txt"), "base\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "-q", "-m", "base"]);
        git(&root, &["remote", "add", "origin", root.to_str().unwrap()]);
        git(&root, &["update-ref", "refs/remotes/origin/main", "HEAD"]);
        git(&root, &["config", "branch.main.remote", "origin"]);
        git(&root, &["config", "branch.main.merge", "refs/heads/main"]);
        fs::write(root.join("tracked.txt"), "ahead\n").unwrap();
        git(&root, &["add", "tracked.txt"]);
        git(&root, &["commit", "-q", "-m", "ahead"]);

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let projected = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(status(&workspace))
            .unwrap();
        assert_eq!(projected.upstream.as_deref(), Some("origin/main"));
        assert_eq!(projected.ahead, 1);
        assert_eq!(projected.behind, 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unborn_repository_diff_includes_staged_and_unstaged_content() {
        let root = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        git(&root, &["init", "-q"]);
        git(&root, &["config", "core.autocrlf", "false"]);
        fs::write(root.join("new.txt"), "staged\n").unwrap();
        git(&root, &["add", "new.txt"]);

        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let resource = workspace.resource("new.txt").unwrap();
        let staged_only = runtime.block_on(file_diff(&workspace, &resource)).unwrap();
        assert_eq!(staged_only.hunks.len(), 1);
        assert_eq!(staged_only.hunks[0].lines.len(), 1);
        assert_eq!(staged_only.hunks[0].lines[0].text, "staged");

        fs::write(root.join("new.txt"), "staged\nunstaged\n").unwrap();
        let combined = runtime.block_on(file_diff(&workspace, &resource)).unwrap();
        assert_eq!(combined.hunks.len(), 1);
        assert_eq!(combined.hunks[0].lines.len(), 2);
        assert_eq!(combined.hunks[0].lines[1].text, "unstaged");
        assert!(
            combined.hunks[0]
                .lines
                .iter()
                .all(|line| line.kind == "added")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn multi_root_status_keeps_two_repositories_and_one_plain_folder_independent() {
        let base = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        let repo_a = base.join("repo-a");
        let repo_b = base.join("repo-b");
        let plain = base.join("plain");
        for root in [&repo_a, &repo_b, &plain] {
            fs::create_dir_all(root).unwrap();
        }
        for (root, content) in [(&repo_a, "a\n"), (&repo_b, "b\n")] {
            git(root, &["init", "-q"]);
            git(root, &["config", "user.email", "sage@example.invalid"]);
            git(root, &["config", "user.name", "Sage Test"]);
            git(root, &["config", "core.autocrlf", "false"]);
            fs::write(root.join("tracked.txt"), content).unwrap();
            git(root, &["add", "tracked.txt"]);
            git(root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("tracked.txt"), format!("{content}changed\n")).unwrap();
        }
        let roots = vec![
            (
                "root-a".into(),
                Ok(Workspace::open("project", "root-a", &repo_a).unwrap()),
            ),
            (
                "root-b".into(),
                Ok(Workspace::open("project", "root-b", &repo_b).unwrap()),
            ),
            (
                "root-plain".into(),
                Ok(Workspace::open("project", "root-plain", &plain).unwrap()),
            ),
        ];

        let result = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(project_status(roots));
        assert_eq!(result.roots.len(), 3);
        assert_eq!(result.roots[0].state, "repository");
        assert_eq!(result.roots[0].changes[0].resource.root_id, "root-a");
        assert_eq!(result.roots[1].state, "repository");
        assert_eq!(result.roots[1].changes[0].resource.root_id, "root-b");
        assert_eq!(result.roots[2].state, "notRepository");
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    #[ignore = "manual release-mode multi-root Git performance evidence"]
    fn benchmarks_twenty_multi_root_git_refreshes() {
        let base = std::env::temp_dir().join(format!("sage-git-bench-{}", Uuid::new_v4()));
        let repo_a = base.join("repo-a");
        let repo_b = base.join("repo-b");
        let plain = base.join("plain");
        for root in [&repo_a, &repo_b, &plain] {
            fs::create_dir_all(root).unwrap();
        }
        for (root, content) in [(&repo_a, "a\n"), (&repo_b, "b\n")] {
            git(root, &["init", "-q"]);
            git(root, &["config", "user.email", "sage@example.invalid"]);
            git(root, &["config", "user.name", "Sage Test"]);
            git(root, &["config", "core.autocrlf", "false"]);
            fs::write(root.join("tracked.txt"), content).unwrap();
            git(root, &["add", "tracked.txt"]);
            git(root, &["commit", "-q", "-m", "fixture"]);
            fs::write(root.join("tracked.txt"), format!("{content}changed\n")).unwrap();
        }
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let mut aggregate = Vec::with_capacity(20);
        let mut root_a_times = Vec::with_capacity(20);
        let mut root_b_times = Vec::with_capacity(20);
        let mut plain_times = Vec::with_capacity(20);
        for _ in 0..20 {
            let roots = vec![
                (
                    "root-a".into(),
                    Ok(Workspace::open("project", "root-a", &repo_a).unwrap()),
                ),
                (
                    "root-b".into(),
                    Ok(Workspace::open("project", "root-b", &repo_b).unwrap()),
                ),
                (
                    "root-plain".into(),
                    Ok(Workspace::open("project", "root-plain", &plain).unwrap()),
                ),
            ];
            let result = runtime.block_on(project_status(roots));
            aggregate.push(result.elapsed_ms);
            root_a_times.push(result.roots[0].elapsed_ms);
            root_b_times.push(result.roots[1].elapsed_ms);
            plain_times.push(result.roots[2].elapsed_ms);
            assert_eq!(result.roots[2].state, "notRepository");
        }
        for samples in [
            &mut aggregate,
            &mut root_a_times,
            &mut root_b_times,
            &mut plain_times,
        ] {
            samples.sort_unstable();
        }
        println!(
            "ADE0005_GIT_MS samples=20 aggregate_p50={} aggregate_p95={} root_a_p50={} root_a_p95={} root_b_p50={} root_b_p95={} plain_p50={} plain_p95={} aggregate_values={aggregate:?}",
            aggregate[9],
            aggregate[18],
            root_a_times[9],
            root_a_times[18],
            root_b_times[9],
            root_b_times[18],
            plain_times[9],
            plain_times[18]
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn nested_root_never_projects_parent_or_sibling_repository_changes() {
        let parent = std::env::temp_dir().join(format!("sage-git-{}", Uuid::new_v4()));
        let nested = parent.join("selected");
        let sibling = parent.join("outside-sibling");
        fs::create_dir_all(&nested).unwrap();
        fs::create_dir_all(&sibling).unwrap();
        git(&parent, &["init", "-q"]);
        git(&parent, &["config", "user.email", "sage@example.invalid"]);
        git(&parent, &["config", "user.name", "Sage Test"]);
        fs::write(nested.join("inside.txt"), "inside\n").unwrap();
        fs::write(sibling.join("outside.txt"), "outside\n").unwrap();
        git(&parent, &["add", "."]);
        git(&parent, &["commit", "-q", "-m", "fixture"]);
        fs::write(parent.join("parent.txt"), "parent change\n").unwrap();
        fs::write(sibling.join("outside.txt"), "sibling change\n").unwrap();

        let workspace = Workspace::open("project", "nested-root", &nested).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let result = runtime.block_on(status(&workspace)).unwrap();
        assert_eq!(result.state, "notRepository");
        assert!(result.changes.is_empty());
        assert!(
            result
                .message
                .as_deref()
                .unwrap()
                .contains("parent Git repository")
        );
        assert!(
            runtime
                .block_on(file_diff(
                    &workspace,
                    &workspace.resource("inside.txt").unwrap(),
                ))
                .is_err()
        );
        fs::remove_dir_all(parent).unwrap();
    }
}
