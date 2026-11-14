use std::{collections::HashMap, time::Instant};

use sage_core::error::{AppError, AppResult};

use crate::{
    RepositoryBoundary, ensure_success,
    parse::{ParsedStatus, merge_numstat, parse_status},
    repository_boundary, repository_diff_base, run_git,
};
use sage_domain::{
    repository::{ProjectRepositoryStatus, RootRepositoryStatus},
    resource::ResourceRef,
};
use sage_workspace::Workspace;

const MAX_STATUS_BYTES: usize = 4 * 1024 * 1024;
pub(crate) type NumstatCounts = (Option<u64>, Option<u64>);
pub(crate) type NumstatByPath = HashMap<String, NumstatCounts>;

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

    let ParsedStatus {
        branch,
        mut changes,
        truncated: parser_truncated,
        overflowed: changes_overflowed,
    } = parse_status(
        workspace.root_id(),
        &status_output.stdout,
        status_output.stdout_truncated,
    )?;
    for change in &mut changes {
        change.resource = workspace.git_resource_identity(&change.resource)?;
        if let Some(original) = &change.original_resource {
            change.original_resource = Some(workspace.git_resource_identity(original)?);
        }
    }
    let needs_numstat = changes
        .iter()
        .any(|change| change.worktree_status != "untracked");
    // Everything below depends only on the status above, so the Git processes
    // run side by side: a refresh waits for the slowest, not for their sum.
    let (tracking, numstat, listed, history, remotes, in_progress) = tokio::try_join!(
        tracking_status(workspace, branch.as_deref()),
        numstat(workspace, needs_numstat),
        crate::branches::list_local_branches(workspace),
        crate::history::list_recent_commits(workspace),
        crate::remotes::list_remotes(workspace),
        crate::in_progress::in_progress(workspace),
    )?;
    let (stats, numstat_truncated) = numstat;
    for change in &mut changes {
        if let Some((additions, deletions)) = stats.get(&change.resource.relative_path) {
            change.additions = *additions;
            change.deletions = *deletions;
        }
    }
    let listed = crate::branches::compose_current_named_branch(listed, branch.as_deref());

    Ok(RootRepositoryStatus {
        root_id: workspace.root_id().into(),
        state: "repository".into(),
        branch,
        upstream: tracking.upstream,
        ahead: tracking.ahead,
        behind: tracking.behind,
        branches: listed.names,
        branches_truncated: listed.truncated,
        recent_commits: history.commits,
        recent_commits_truncated: history.truncated,
        remotes: remotes.names,
        remotes_truncated: remotes.truncated,
        merge_in_progress: in_progress.merge,
        rebase_in_progress: in_progress.rebase,
        cherry_pick_in_progress: in_progress.cherry_pick,
        revert_in_progress: in_progress.revert,
        changes,
        truncated: parser_truncated || numstat_truncated,
        changes_overflowed,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: None,
    })
}

pub(crate) struct TrackingStatus {
    pub(crate) upstream: Option<String>,
    pub(crate) ahead: u64,
    pub(crate) behind: u64,
}

pub(crate) async fn tracking_status(
    workspace: &Workspace,
    branch: Option<&str>,
) -> AppResult<TrackingStatus> {
    if crate::branches::named_branch(branch).is_none() {
        return Ok(TrackingStatus {
            upstream: None,
            ahead: 0,
            behind: 0,
        });
    }
    let upstream = crate::workspace::run_git(
        workspace,
        &[
            "rev-parse",
            "--abbrev-ref",
            "--symbolic-full-name",
            "@{upstream}",
        ],
        1024,
    )
    .await?;
    if !upstream.succeeded() && matches!(upstream.exit_code(), Some(1) | Some(128)) {
        return Ok(TrackingStatus {
            upstream: None,
            ahead: 0,
            behind: 0,
        });
    }
    ensure_success(&upstream)?;
    if upstream.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git upstream identity exceeds its bounded contract".into(),
        ));
    }
    let upstream = std::str::from_utf8(&upstream.stdout)
        .map_err(|_| AppError::Git("Git returned a non-UTF-8 upstream identity".into()))?
        .trim();
    if upstream.is_empty() || upstream.len() > 512 || upstream.chars().any(char::is_control) {
        return Err(AppError::Git(
            "Git returned an invalid upstream identity".into(),
        ));
    }
    let counts = crate::workspace::run_git(
        workspace,
        &["rev-list", "--left-right", "--count", "@{upstream}...HEAD"],
        128,
    )
    .await?;
    ensure_success(&counts)?;
    if counts.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git tracking counts exceed their bounded contract".into(),
        ));
    }
    let counts = std::str::from_utf8(&counts.stdout)
        .map_err(|_| AppError::Git("Git returned non-UTF-8 tracking counts".into()))?
        .split_whitespace()
        .map(str::parse::<u64>)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| AppError::Git("Git returned invalid tracking counts".into()))?;
    let [behind, ahead] = counts.as_slice() else {
        return Err(AppError::Git(
            "Git returned incomplete tracking counts".into(),
        ));
    };
    Ok(TrackingStatus {
        upstream: Some(upstream.into()),
        ahead: *ahead,
        behind: *behind,
    })
}

pub(crate) fn canonicalize_numstat_paths(
    workspace: &Workspace,
    stats: NumstatByPath,
) -> AppResult<NumstatByPath> {
    let mut canonical = HashMap::with_capacity(stats.len());
    for (relative_path, counts) in stats {
        let resource = workspace.git_resource_identity(&ResourceRef {
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

pub async fn project_status(
    roots: Vec<(String, Result<Workspace, String>)>,
) -> ProjectRepositoryStatus {
    let started = Instant::now();
    let mut results = vec![None; roots.len()];
    let mut tasks = Vec::with_capacity(roots.len());
    for (index, (root_id, workspace)) in roots.into_iter().enumerate() {
        match workspace {
            Ok(workspace) => tasks.push((
                index,
                root_id,
                tokio::spawn(async move { status(&workspace).await }),
            )),
            Err(message) => {
                results[index] = Some(RootRepositoryStatus::unavailable(root_id, message))
            }
        }
    }
    for (index, root_id, task) in tasks {
        results[index] = Some(match task.await {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => RootRepositoryStatus::unavailable(root_id, error.to_string()),
            Err(error) => RootRepositoryStatus::unavailable(
                root_id,
                format!("Git status task failed: {error}"),
            ),
        });
    }
    let roots = results.into_iter().flatten().collect::<Vec<_>>();
    ProjectRepositoryStatus {
        truncated: roots.iter().any(|root| root.truncated),
        roots,
        elapsed_ms: started.elapsed().as_millis() as u64,
    }
}

/// Lines added and removed per changed File, against `HEAD` (or the empty
/// tree in a repository with no commits yet).
async fn numstat(workspace: &Workspace, needed: bool) -> AppResult<(NumstatByPath, bool)> {
    if !needed {
        return Ok((HashMap::new(), false));
    }
    let base = repository_diff_base(workspace.root()).await?;
    let output = run_git(
        workspace.root(),
        &[
            "diff",
            "--numstat",
            "-z",
            "--no-ext-diff",
            "--no-textconv",
            &base,
        ],
        MAX_STATUS_BYTES,
    )
    .await?;
    ensure_success(&output)?;
    let mut stats = HashMap::new();
    let truncated = merge_numstat(&mut stats, &output.stdout, output.stdout_truncated)?;
    Ok((canonicalize_numstat_paths(workspace, stats)?, truncated))
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn status_keeps_working_with_non_utf8_tracked_and_untracked_paths() {
        use super::*;
        use std::{ffi::OsStr, fs, os::unix::ffi::OsStrExt};

        let root = std::env::temp_dir().join(format!("sage-status-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let git = |args: &[&str]| {
            let output = crate::process::fixture_git()
                .arg("-C")
                .arg(&root)
                .args(args)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        };
        git(&["init", "-q", "-b", "main"]);
        let tracked = root.join(OsStr::from_bytes(b"tracked-\xff.txt"));
        fs::write(&tracked, "before\n").unwrap();
        fs::write(root.join("ordinary.txt"), "before\n").unwrap();
        git(&["add", "."]);
        git(&[
            "-c",
            "user.name=Sage Test",
            "-c",
            "user.email=test",
            "commit",
            "-qm",
            "initial",
        ]);
        fs::write(&tracked, "after\n").unwrap();
        fs::write(root.join(OsStr::from_bytes(b"untracked-\xff.txt")), "new\n").unwrap();
        fs::write(root.join("ordinary.txt"), "after\n").unwrap();
        let workspace = Workspace::open("project-test", "root-test", &root).unwrap();
        let result = status(&workspace).await;
        fs::remove_dir_all(&root).unwrap();
        let result = result.unwrap();
        assert_eq!(result.state, "repository");
        assert_eq!(result.branch.as_deref(), Some("main"));
        assert_eq!(result.recent_commits.len(), 1);
        assert_eq!(result.changes.len(), 1);
        assert_eq!(result.changes[0].resource.relative_path, "ordinary.txt");
        assert_eq!(result.changes[0].worktree_status, "modified");
        assert_eq!(result.changes[0].additions, Some(1));
        assert_eq!(result.changes[0].deletions, Some(1));
        assert!(!result.truncated);
        assert!(!result.changes_overflowed);
    }
}
