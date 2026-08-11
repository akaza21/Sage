use std::{
    collections::HashMap,
    path::Path,
    time::{Duration, Instant},
};

use crate::{
    domain::{FileDiff, ProjectRepositoryStatus, ResourceRef, RootRepositoryStatus},
    error::{AppError, AppResult},
    git_process,
    repository_parse::{
        MAX_DIFF_LINES, added_file_diff, merge_numstat, parse_status, parse_unified_diff,
    },
    workspace::Workspace,
};

const GIT_TIMEOUT: Duration = Duration::from_secs(3);
const MAX_STATUS_BYTES: usize = 4 * 1024 * 1024;
const MAX_DIFF_BYTES: usize = 1024 * 1024;
type NumstatCounts = (Option<u64>, Option<u64>);
type NumstatByPath = HashMap<String, NumstatCounts>;

#[cfg(test)]
pub(crate) use git_process::GitProcessTestHarness;
pub(crate) use git_process::{GitOutput, GitProcessPolicy};

impl GitOutput {
    pub(crate) fn succeeded(&self) -> bool {
        self.status.success()
    }

    pub(crate) fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }
}

enum RepositoryBoundary {
    Exact,
    Nested,
    NotRepository,
}

pub async fn status(workspace: &Workspace) -> AppResult<RootRepositoryStatus> {
    let started = Instant::now();
    match repository_boundary(workspace.root()).await? {
        RepositoryBoundary::Exact => {}
        RepositoryBoundary::Nested => {
            return Ok(RootRepositoryStatus::repository_subdirectory(
                workspace.root_id(),
                started.elapsed().as_millis() as u64,
            ));
        }
        RepositoryBoundary::NotRepository => {
            return Ok(RootRepositoryStatus::not_repository(
                workspace.root_id(),
                started.elapsed().as_millis() as u64,
            ));
        }
    }
    let status_output = run_git(
        workspace.root(),
        &[
            "status",
            "--branch",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
        ],
        MAX_STATUS_BYTES,
    )
    .await?;
    ensure_success(&status_output)?;

    let (branch, mut changes, parser_truncated) = parse_status(
        workspace.root_id(),
        &status_output.stdout,
        status_output.stdout_truncated,
    )?;
    for change in &mut changes {
        change.resource = workspace.canonical_ingress_resource(&change.resource)?;
        if let Some(original) = &change.original_resource {
            change.original_resource = Some(workspace.canonical_ingress_resource(original)?);
        }
    }
    let mut stats = HashMap::new();
    let mut numstat_truncated = false;
    let needs_numstat = changes
        .iter()
        .any(|change| change.worktree_status != "untracked");
    if needs_numstat {
        let base = repository_diff_base(workspace.root()).await?;
        let output = run_git(
            workspace.root(),
            &["diff", "--numstat", "-z", "--no-ext-diff", &base],
            MAX_STATUS_BYTES,
        )
        .await?;
        ensure_success(&output)?;
        numstat_truncated = merge_numstat(&mut stats, &output.stdout, output.stdout_truncated)?;
        stats = canonicalize_numstat_paths(workspace, stats)?;
    }

    for change in &mut changes {
        if let Some((additions, deletions)) = stats.get(&change.resource.relative_path) {
            change.additions = *additions;
            change.deletions = *deletions;
        }
    }

    Ok(RootRepositoryStatus {
        root_id: workspace.root_id().into(),
        state: "repository".into(),
        branch,
        changes,
        truncated: parser_truncated || numstat_truncated,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: None,
    })
}

fn canonicalize_numstat_paths(
    workspace: &Workspace,
    stats: NumstatByPath,
) -> AppResult<NumstatByPath> {
    let mut canonical = HashMap::with_capacity(stats.len());
    for (relative_path, counts) in stats {
        let resource = workspace.canonical_ingress_resource(&ResourceRef {
            root_id: workspace.root_id().into(),
            relative_path,
        })?;
        if canonical.insert(resource.relative_path, counts).is_some() {
            return Err(AppError::Git(
                "Git numstat returned duplicate canonical resource identities".into(),
            ));
        }
    }
    Ok(canonical)
}

pub async fn file_diff(workspace: &Workspace, resource: &ResourceRef) -> AppResult<FileDiff> {
    if !matches!(
        repository_boundary(workspace.root()).await?,
        RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "diffs require a selected root that equals the Git top level".into(),
        ));
    }
    let relative_path = workspace.validate_resource(resource, false)?;
    let resource = ResourceRef {
        root_id: workspace.root_id().into(),
        relative_path: relative_path.clone(),
    };
    let base = repository_diff_base(workspace.root()).await?;
    let mut args = vec!["diff", "--no-ext-diff", "--no-color", "--unified=3"];
    args.push(&base);
    args.extend(["--", relative_path.as_str()]);
    let output = run_git(workspace.root(), &args, MAX_DIFF_BYTES).await?;
    ensure_success(&output)?;

    if output.stdout.is_empty() && is_untracked(workspace.root(), &relative_path).await? {
        let document = workspace.read_text_file(&resource)?;
        return Ok(added_file_diff(
            resource,
            &document.content,
            document.content.lines().count() > MAX_DIFF_LINES,
        ));
    }

    parse_unified_diff(
        &resource,
        &String::from_utf8_lossy(&output.stdout),
        output.stdout_truncated,
    )
}

pub async fn project_status(
    roots: Vec<(String, Result<Workspace, String>)>,
) -> ProjectRepositoryStatus {
    let started = Instant::now();
    let mut results = vec![None; roots.len()];
    let mut tasks = Vec::with_capacity(roots.len());
    for (index, (root_id, workspace)) in roots.into_iter().enumerate() {
        match workspace {
            Ok(workspace) => {
                let task = tokio::spawn(async move { status(&workspace).await });
                tasks.push((index, root_id, task));
            }
            Err(message) => {
                results[index] = Some(RootRepositoryStatus::unavailable(root_id, message));
            }
        }
    }
    for (index, root_id, task) in tasks {
        match task.await {
            Ok(Ok(status)) => results[index] = Some(status),
            Ok(Err(error)) => {
                results[index] = Some(RootRepositoryStatus::unavailable(
                    root_id,
                    error.to_string(),
                ));
            }
            Err(error) => {
                results[index] = Some(RootRepositoryStatus::unavailable(
                    root_id,
                    format!("Git status task failed: {error}"),
                ));
            }
        }
    }
    let roots = results.into_iter().flatten().collect::<Vec<_>>();
    ProjectRepositoryStatus {
        truncated: roots.iter().any(|root| root.truncated),
        roots,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

async fn repository_diff_base(root: &Path) -> AppResult<String> {
    let head = run_git(root, &["rev-parse", "--verify", "--quiet", "HEAD"], 256).await?;
    if head.status.success() {
        return Ok("HEAD".into());
    }

    let empty_tree = run_git(root, &["hash-object", "-t", "tree", "--stdin"], 256).await?;
    ensure_success(&empty_tree)?;
    let oid = std::str::from_utf8(&empty_tree.stdout)
        .map_err(|_| AppError::Git("Git returned a non-UTF-8 object id".into()))?
        .trim();
    if oid.is_empty() {
        return Err(AppError::Git("Git returned an empty tree object id".into()));
    }
    Ok(oid.into())
}

async fn is_untracked(root: &Path, relative_path: &str) -> AppResult<bool> {
    let output = run_git(
        root,
        &[
            "status",
            "--porcelain=v2",
            "-z",
            "--untracked-files=all",
            "--",
            relative_path,
        ],
        4 * 1024,
    )
    .await?;
    ensure_success(&output)?;
    Ok(output.stdout.starts_with(b"? "))
}

async fn repository_boundary(root: &Path) -> AppResult<RepositoryBoundary> {
    let output = run_git(root, &["rev-parse", "--show-toplevel"], 32 * 1024).await?;
    if !output.status.success() {
        return Ok(RepositoryBoundary::NotRepository);
    }
    let top_level = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git top-level path is not valid UTF-8".into()))?
        .trim();
    if top_level.is_empty() {
        return Err(AppError::Git("Git returned an empty top-level path".into()));
    }
    let top_level = Path::new(top_level).canonicalize()?;
    if top_level == root {
        Ok(RepositoryBoundary::Exact)
    } else {
        Ok(RepositoryBoundary::Nested)
    }
}

pub(crate) async fn run_git(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
) -> AppResult<GitOutput> {
    match run_git_with_policy(
        root,
        args,
        stdout_limit,
        GitProcessPolicy::read(GIT_TIMEOUT),
    )
    .await
    {
        Err(AppError::GitEffectUnknown(message)) => Err(AppError::Git(message)),
        result => result,
    }
}

pub(crate) async fn run_git_with_policy(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    git_process::run(root, args, stdout_limit, policy).await
}

pub(crate) fn ensure_success(output: &GitOutput) -> AppResult<()> {
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(AppError::Git(message.trim().to_owned()))
}

#[cfg(test)]
mod tests {
    use std::{fs, process::Command as StdCommand};

    use uuid::Uuid;

    use super::*;

    fn git(root: &Path, args: &[&str]) {
        assert!(
            StdCommand::new("git")
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
