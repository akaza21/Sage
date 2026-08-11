use std::time::Instant;

use crate::{
    domain::{FileChange, ProjectRepositoryStatus, ResourceRef, RootRepositoryStatus},
    error::{AppError, AppResult},
    repository::{self, RepositoryBoundary},
    workspace::Workspace,
};

const MAX_INDEX_STATUS_BYTES: usize = 4 * 1024 * 1024;
const MAX_CHANGES: usize = 300;
const INDEX_ONLY_MESSAGE: &str = "Commit completed with an index-only projection. Refresh Source Control to inspect worktree changes.";

pub(crate) async fn project_status(
    roots: Vec<(String, Result<Workspace, String>)>,
) -> ProjectRepositoryStatus {
    let started = Instant::now();
    let mut results = vec![None; roots.len()];
    let mut tasks = Vec::with_capacity(roots.len());
    for (index, (root_id, workspace)) in roots.into_iter().enumerate() {
        match workspace {
            Ok(workspace) => {
                tasks.push((
                    index,
                    root_id,
                    tokio::spawn(async move { status(&workspace).await }),
                ));
            }
            Err(message) => {
                results[index] = Some(RootRepositoryStatus::unavailable(root_id, message));
            }
        }
    }
    for (index, root_id, task) in tasks {
        results[index] = Some(match task.await {
            Ok(Ok(status)) => status,
            Ok(Err(error)) => RootRepositoryStatus::unavailable(root_id, error.to_string()),
            Err(error) => RootRepositoryStatus::unavailable(
                root_id,
                format!("Git index status task failed: {error}"),
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

async fn status(workspace: &Workspace) -> AppResult<RootRepositoryStatus> {
    let started = Instant::now();
    match repository::repository_boundary(workspace.root()).await? {
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

    let branch = symbolic_branch(workspace).await?;
    let base = repository::repository_diff_base(workspace.root()).await?;
    let output = repository::run_git(
        workspace.root(),
        &[
            "diff",
            "--cached",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            "--no-textconv",
            &base,
            "--",
        ],
        MAX_INDEX_STATUS_BYTES,
    )
    .await?;
    repository::ensure_success(&output)?;
    let changes = parse_changes(workspace, &output.stdout, output.stdout_truncated)?;

    Ok(RootRepositoryStatus {
        root_id: workspace.root_id().into(),
        state: "repository".into(),
        branch,
        changes,
        truncated: true,
        elapsed_ms: started.elapsed().as_millis() as u64,
        message: Some(INDEX_ONLY_MESSAGE.into()),
    })
}

async fn symbolic_branch(workspace: &Workspace) -> AppResult<Option<String>> {
    let output = repository::run_git(
        workspace.root(),
        &["symbolic-ref", "--quiet", "--short", "HEAD"],
        1024,
    )
    .await?;
    if output.exit_code() == Some(1) {
        return Ok(Some("HEAD (detached)".into()));
    }
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git branch name exceeds its bounded contract".into(),
        ));
    }
    let branch = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git returned a non-UTF-8 branch name".into()))?
        .trim_end_matches(['\r', '\n']);
    if branch.is_empty() || branch.chars().any(char::is_control) {
        return Err(AppError::Git("Git returned an invalid branch name".into()));
    }
    Ok(Some(branch.into()))
}

fn parse_changes(
    workspace: &Workspace,
    bytes: &[u8],
    output_truncated: bool,
) -> AppResult<Vec<FileChange>> {
    let complete = if output_truncated {
        bytes
            .iter()
            .rposition(|byte| *byte == 0)
            .map_or(&[][..], |last| &bytes[..=last])
    } else {
        if !bytes.is_empty() && !bytes.ends_with(&[0]) {
            return Err(AppError::Git(
                "Git index status record is incomplete".into(),
            ));
        }
        bytes
    };
    let mut fields = complete.split(|byte| *byte == 0);
    let mut changes = Vec::new();
    while let Some(status) = fields.next() {
        if status.is_empty() {
            break;
        }
        let Some(path) = fields.next() else {
            if output_truncated {
                break;
            }
            return Err(AppError::Git(
                "Git index status record is incomplete".into(),
            ));
        };
        if path.is_empty() {
            return Err(AppError::Git("Git returned an empty index path".into()));
        }
        if changes.len() == MAX_CHANGES {
            break;
        }
        let status = std::str::from_utf8(status)
            .map_err(|_| AppError::Git("Git returned a non-UTF-8 index status".into()))?;
        let relative_path = std::str::from_utf8(path)
            .map_err(|_| AppError::Git("Git returned a non-UTF-8 index path".into()))?;
        let resource = workspace.git_resource_identity(&ResourceRef {
            root_id: workspace.root_id().into(),
            relative_path: relative_path.into(),
        })?;
        changes.push(FileChange {
            resource,
            original_resource: None,
            index_status: index_status(status).into(),
            worktree_status: "unknown".into(),
            additions: None,
            deletions: None,
        });
    }
    Ok(changes)
}

fn index_status(status: &str) -> &'static str {
    match status.as_bytes().first() {
        Some(b'A') => "added",
        Some(b'D') => "deleted",
        Some(b'M' | b'T') => "modified",
        Some(b'U') => "conflicted",
        _ => "unknown",
    }
}
