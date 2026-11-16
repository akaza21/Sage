use sage_core::error::{AppError, AppResult};

use crate::{
    git_budget,
    repository::{self, RepositoryBoundary},
};
use sage_workspace::Workspace;

pub(crate) const MAX_GIT_COMMIT_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_GIT_IDENTITY_BYTES: usize = 512;
const MAX_GIT_CONNECTIVITY_BYTES: usize = 4 * 1024 * 1024;
const MAX_GIT_OID_BYTES: usize = 128;
const MAX_GIT_REF_BYTES: usize = 4 * 1024;
struct PreparedCommit {
    head_ref: String,
    parent_oid: Option<String>,
    tree_oid: String,
}

pub(crate) async fn commit_staged(workspace: &Workspace, message: &str) -> AppResult<()> {
    commit_staged_with_policy(
        workspace,
        message,
        repository::GitProcessPolicy::mutation(git_budget::MUTATION),
    )
    .await
}

#[cfg(test)]
pub(crate) async fn commit_staged_with_timeout(
    workspace: &Workspace,
    message: &str,
    mutation_timeout: std::time::Duration,
) -> AppResult<()> {
    // A zero test deadline must fail before any preflight can consume a fresh
    // mutation budget. This keeps the containment regression deterministic
    // under a loaded serial suite and never weakens production timeouts.
    if mutation_timeout.is_zero() {
        return Err(AppError::GitEffectUnknown(
            "mutation deadline elapsed before admission".into(),
        ));
    }
    commit_staged_with_policy(
        workspace,
        message,
        repository::GitProcessPolicy::mutation(mutation_timeout),
    )
    .await
}

pub(crate) async fn commit_staged_with_policy(
    workspace: &Workspace,
    message: &str,
    process_policy: repository::GitProcessPolicy,
) -> AppResult<()> {
    let message = normalize_message(message)?;
    if !matches!(
        crate::repository_workspace::repository_boundary(workspace).await?,
        RepositoryBoundary::Exact
    ) {
        return Err(AppError::InvalidRequest(
            "Git commit requires a Project root that equals the repository top level".into(),
        ));
    }
    let pre_ref_policy = process_policy.pre_ref_mutation(git_budget::MUTATION);
    let head_ref = symbolic_head_ref(workspace).await?;
    reject_unfinished_history(workspace).await?;
    let unmerged =
        crate::repository_workspace::run_git(workspace, &["ls-files", "--unmerged", "-z"], 1)
            .await?;
    repository::ensure_success(&unmerged)?;
    if !unmerged.stdout.is_empty() || unmerged.stdout_truncated {
        return Err(AppError::InvalidRequest(
            "conflicted changes require the later conflict workflow".into(),
        ));
    }
    let merge_parent = crate::repository_merge_commit::admit_merge_parent_oid(workspace).await?;
    let prepared = prepare_index(
        workspace,
        head_ref,
        pre_ref_policy.clone(),
        merge_parent.is_some(),
    )
    .await?;

    let name = local_identity_value(workspace, "user.name").await?;
    let email = local_identity_value(workspace, "user.email").await?;
    let name_config = format!("user.name={name}");
    let email_config = format!("user.email={email}");
    let mut commit_arguments = vec![
        "-c",
        name_config.as_str(),
        "-c",
        email_config.as_str(),
        "-c",
        "user.useConfigOnly=true",
        "-c",
        "commit.gpgSign=false",
        "commit-tree",
        prepared.tree_oid.as_str(),
    ];
    if let Some(parent_oid) = prepared.parent_oid.as_deref() {
        commit_arguments.extend(["-p", parent_oid]);
    }
    if let Some(merge_oid) = merge_parent.as_deref() {
        commit_arguments.extend(["-p", merge_oid]);
    }
    commit_arguments.extend(["-m", message.as_str()]);
    let commit = crate::repository_workspace::run_git_with_policy(
        workspace,
        &commit_arguments,
        MAX_GIT_OID_BYTES,
        pre_ref_policy,
    )
    .await?;
    repository::ensure_success(&commit)?;
    let commit_oid = bounded_oid(&commit, "commit")?;
    let expected_old = prepared
        .parent_oid
        .unwrap_or_else(|| "0".repeat(commit_oid.len()));
    let output = crate::repository_workspace::run_git_with_policy(
        workspace,
        &[
            "update-ref",
            "-m",
            "commit: Sage",
            prepared.head_ref.as_str(),
            commit_oid.as_str(),
            expected_old.as_str(),
        ],
        64 * 1024,
        process_policy,
    )
    .await?;
    repository::ensure_success(&output)?;
    if merge_parent.is_some() {
        crate::repository_merge_commit::clear_merge_state(workspace).await?;
    }
    Ok(())
}

async fn symbolic_head_ref(workspace: &Workspace) -> AppResult<String> {
    let output = crate::repository_workspace::run_git(
        workspace,
        &["symbolic-ref", "--quiet", "HEAD"],
        MAX_GIT_REF_BYTES + 2,
    )
    .await?;
    if output.exit_code() == Some(1) {
        return Err(AppError::InvalidRequest(
            "detached HEAD requires the later Git workflow".into(),
        ));
    }
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git HEAD reference exceeds its bounded contract".into(),
        ));
    }
    let head_ref = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git returned a non-UTF-8 HEAD reference".into()))?
        .trim_end_matches(['\r', '\n']);
    if !head_ref.starts_with("refs/heads/")
        || head_ref.len() > MAX_GIT_REF_BYTES
        || head_ref.chars().any(char::is_control)
    {
        return Err(AppError::Git(
            "Git returned an invalid bounded branch reference".into(),
        ));
    }
    Ok(head_ref.into())
}

async fn prepare_index(
    workspace: &Workspace,
    head_ref: String,
    pre_ref_policy: repository::GitProcessPolicy,
    allow_empty_tree: bool,
) -> AppResult<PreparedCommit> {
    let tree = crate::repository_workspace::run_git_with_policy(
        workspace,
        &["write-tree"],
        MAX_GIT_OID_BYTES,
        pre_ref_policy,
    )
    .await?;
    repository::ensure_success(&tree)?;
    let tree_oid = bounded_oid(&tree, "tree")?;
    let connectivity = crate::repository_workspace::run_git(
        workspace,
        &[
            "rev-list",
            "--objects",
            "--missing=print",
            tree_oid.as_str(),
        ],
        MAX_GIT_CONNECTIVITY_BYTES,
    )
    .await?;
    repository::ensure_success(&connectivity)?;
    if connectivity.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git staged-index connectivity exceeds its bounded contract".into(),
        ));
    }
    if connectivity
        .stdout
        .split(|byte| *byte == b'\n')
        .any(|line| line.starts_with(b"?"))
    {
        return Err(AppError::InvalidRequest(
            "Git staged index references a missing object".into(),
        ));
    }

    let parent = crate::repository_workspace::run_git(
        workspace,
        &["rev-parse", "--verify", "--quiet", head_ref.as_str()],
        MAX_GIT_OID_BYTES,
    )
    .await?;
    let parent_oid = if parent.exit_code() == Some(1) {
        None
    } else {
        repository::ensure_success(&parent)?;
        Some(bounded_oid(&parent, "parent")?)
    };

    if let Some(parent_oid) = parent_oid.as_deref() {
        let parent_tree_expression = format!("{parent_oid}^{{tree}}");
        let parent_tree = crate::repository_workspace::run_git(
            workspace,
            &["rev-parse", "--verify", parent_tree_expression.as_str()],
            MAX_GIT_OID_BYTES,
        )
        .await?;
        repository::ensure_success(&parent_tree)?;
        if !allow_empty_tree && bounded_oid(&parent_tree, "parent tree")? == tree_oid {
            return Err(AppError::InvalidRequest(
                "Git commit requires at least one staged change".into(),
            ));
        }
    } else {
        let initial_tree = crate::repository_workspace::run_git(
            workspace,
            &["ls-tree", "--name-only", "-z", tree_oid.as_str()],
            1,
        )
        .await?;
        repository::ensure_success(&initial_tree)?;
        if initial_tree.stdout.is_empty() && !initial_tree.stdout_truncated {
            return Err(AppError::InvalidRequest(
                "Git commit requires at least one staged change".into(),
            ));
        }
    }

    Ok(PreparedCommit {
        head_ref,
        parent_oid,
        tree_oid,
    })
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

pub(crate) fn normalize_message(message: &str) -> AppResult<String> {
    let normalized = message.trim();
    if normalized.is_empty()
        || normalized.len() > MAX_GIT_COMMIT_MESSAGE_BYTES
        || normalized.contains('\0')
    {
        return Err(AppError::InvalidRequest(format!(
            "Git commit message must contain 1 to {MAX_GIT_COMMIT_MESSAGE_BYTES} UTF-8 bytes and no NUL"
        )));
    }
    Ok(normalized.into())
}

async fn local_identity_value(workspace: &Workspace, key: &str) -> AppResult<String> {
    let output = crate::repository_workspace::run_git(
        workspace,
        &["config", "--local", "--no-includes", "--get", key],
        MAX_GIT_IDENTITY_BYTES + 2,
    )
    .await?;
    if output.exit_code() == Some(1) {
        return Err(AppError::InvalidRequest(format!(
            "Git commit requires repository-local {key}"
        )));
    }
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::InvalidRequest(format!(
            "repository-local {key} exceeds its bounded contract"
        )));
    }
    let value = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::InvalidRequest(format!("repository-local {key} is not UTF-8")))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty()
        || value.len() > MAX_GIT_IDENTITY_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(AppError::InvalidRequest(format!(
            "repository-local {key} is not a valid bounded identity"
        )));
    }
    Ok(value.into())
}

async fn reject_unfinished_history(workspace: &Workspace) -> AppResult<()> {
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
    Ok(())
}

#[cfg(test)]
#[path = "repository_commit_tests.rs"]
mod tests;
