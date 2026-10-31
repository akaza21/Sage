use std::{collections::HashMap, time::Instant};

use crate::{
    domain::{ProjectRepositoryStatus, ResourceRef, RootRepositoryStatus},
    error::{AppError, AppResult},
    repository::{
        RepositoryBoundary, ensure_success, repository_boundary, repository_diff_base, run_git,
    },
    repository_parse::{merge_numstat, parse_status},
    workspace::Workspace,
};

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

    let (branch, mut changes, parser_truncated) = parse_status(
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
        crate::repository_branches::list_local_branches(workspace),
        crate::repository_history::list_recent_commits(workspace),
        crate::repository_remotes::list_remotes(workspace),
        crate::repository_in_progress::in_progress(workspace),
    )?;
    let (stats, numstat_truncated) = numstat;
    for change in &mut changes {
        if let Some((additions, deletions)) = stats.get(&change.resource.relative_path) {
            change.additions = *additions;
            change.deletions = *deletions;
        }
    }
    let listed =
        crate::repository_branches::compose_current_named_branch(listed, branch.as_deref());

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
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: None,
    })
}

pub(crate) struct TrackingStatus {
    pub upstream: Option<String>,
    pub ahead: u64,
    pub behind: u64,
}

pub(crate) async fn tracking_status(
    workspace: &Workspace,
    branch: Option<&str>,
) -> AppResult<TrackingStatus> {
    if crate::repository_branches::named_branch(branch).is_none() {
        return Ok(TrackingStatus {
            upstream: None,
            ahead: 0,
            behind: 0,
        });
    }
    let upstream = crate::repository_workspace::run_git(
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
    let counts = crate::repository_workspace::run_git(
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
