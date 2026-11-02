use std::path::Path;

use crate::{
    domain::{FileDiff, ResourceRef},
    error::{AppError, AppResult},
    repository::{RepositoryBoundary, ensure_success, repository_boundary, run_git},
    repository_parse::{MAX_DIFF_LINES, added_file_diff, parse_unified_diff},
    workspace::Workspace,
};

const MAX_DIFF_BYTES: usize = 1024 * 1024;

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
    let mut args = vec![
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--no-color",
        "--unified=3",
    ];
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

pub(crate) async fn repository_diff_base(root: &Path) -> AppResult<String> {
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
