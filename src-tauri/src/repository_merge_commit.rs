use std::path::PathBuf;

use crate::{
    error::{AppError, AppResult},
    repository,
    workspace::Workspace,
};

const MAX_GIT_ADMIN_PATH_BYTES: usize = 32 * 1024;
const MAX_GIT_OID_BYTES: usize = 128;

pub(crate) async fn admit_merge_parent_oid(workspace: &Workspace) -> AppResult<Option<String>> {
    if !crate::repository_conflict::admin_marker_exists(workspace, "MERGE_HEAD").await? {
        return Ok(None);
    }
    for marker in [
        "CHERRY_PICK_HEAD",
        "REVERT_HEAD",
        "REBASE_HEAD",
        "rebase-merge",
        "rebase-apply",
    ] {
        if crate::repository_conflict::admin_marker_exists(workspace, marker).await? {
            return Err(AppError::InvalidRequest(
                "unfinished merge, cherry-pick, revert, or rebase requires the later Git workflow"
                    .into(),
            ));
        }
    }
    let merge = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--verify", "--quiet", "MERGE_HEAD"],
        MAX_GIT_OID_BYTES,
    )
    .await?;
    if merge.exit_code() == Some(1) {
        return Err(AppError::Git(
            "Git returned an invalid MERGE_HEAD object identity".into(),
        ));
    }
    repository::ensure_success(&merge)?;
    let merge_oid = bounded_oid(&merge, "MERGE_HEAD")?;
    let head = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--verify", "--quiet", "HEAD"],
        MAX_GIT_OID_BYTES,
    )
    .await?;
    if head.exit_code() == Some(1) {
        return Err(AppError::InvalidRequest(
            "in-progress merge requires an existing HEAD".into(),
        ));
    }
    repository::ensure_success(&head)?;
    let head_oid = bounded_oid(&head, "HEAD")?;
    if merge_oid == head_oid {
        return Err(AppError::InvalidRequest(
            "in-progress merge parent is not distinct".into(),
        ));
    }
    Ok(Some(merge_oid))
}

pub(crate) async fn clear_merge_state(workspace: &Workspace) -> AppResult<()> {
    remove_admin_regular_file(workspace, "MERGE_HEAD", true).await?;
    for marker in ["MERGE_MSG", "MERGE_MODE", "AUTO_MERGE"] {
        remove_admin_regular_file(workspace, marker, false).await?;
    }
    Ok(())
}

async fn remove_admin_regular_file(
    workspace: &Workspace,
    marker: &str,
    required: bool,
) -> AppResult<()> {
    let output = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--path-format=absolute", "--git-path", marker],
        MAX_GIT_ADMIN_PATH_BYTES + 2,
    )
    .await?;
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git administrative path exceeds its bounded contract".into(),
        ));
    }
    let path = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git administrative path is not UTF-8".into()))?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty() || path.contains(['\r', '\n', '\0']) {
        return Err(AppError::Git(
            "Git returned an invalid administrative path".into(),
        ));
    }
    let path = PathBuf::from(path);
    if !path.is_absolute() {
        return Err(AppError::Git(
            "Git returned a non-absolute administrative path".into(),
        ));
    }
    match std::fs::symlink_metadata(&path) {
        Ok(metadata) if metadata.is_file() => std::fs::remove_file(path).map_err(Into::into),
        Ok(_) => Err(AppError::Git(format!(
            "Git administrative path {marker} is not a regular file"
        ))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound && !required => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(AppError::Git(format!(
            "Git administrative path {marker} is missing"
        ))),
        Err(error) => Err(error.into()),
    }
}

fn bounded_oid(output: &repository::GitOutput, label: &str) -> AppResult<String> {
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(format!(
            "Git {label} object identity exceeds its bounded contract"
        )));
    }
    let oid = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git(format!("Git returned a non-UTF-8 {label} object identity")))?
        .trim_end_matches(['\r', '\n']);
    if !(40..=MAX_GIT_OID_BYTES).contains(&oid.len())
        || !oid.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(AppError::Git(format!(
            "Git returned an invalid {label} object identity"
        )));
    }
    Ok(oid.into())
}
