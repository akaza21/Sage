use std::path::Path;

use sage_core::error::{AppError, AppResult, RepositoryLayout};

use crate::{budget, process};

pub(crate) use process::{GitOutput, GitProcessPolicy};

impl GitOutput {
    pub fn succeeded(&self) -> bool {
        self.status.success()
    }

    pub fn exit_code(&self) -> Option<i32> {
        self.status.code()
    }
}

pub enum RepositoryBoundary {
    Exact,
    Nested,
    NotRepository,
}

pub async fn repository_boundary(root: &Path) -> AppResult<RepositoryBoundary> {
    let output = match run_git(root, &["rev-parse", "--show-toplevel"], 32 * 1024).await {
        Err(AppError::RepositoryOutsideRoot(RepositoryLayout::NestedRoot)) => {
            return Ok(RepositoryBoundary::Nested);
        }
        output => output?,
    };
    repository_boundary_from_output(root, &output)
}

pub(crate) fn repository_boundary_from_output(
    root: &Path,
    output: &GitOutput,
) -> AppResult<RepositoryBoundary> {
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

pub async fn run_git(root: &Path, args: &[&str], stdout_limit: usize) -> AppResult<GitOutput> {
    match run_git_with_policy(
        root,
        args,
        stdout_limit,
        GitProcessPolicy::read(budget::READ),
    )
    .await
    {
        Err(AppError::GitEffectUnknown(message)) => Err(AppError::Git(message)),
        result => result,
    }
}

pub async fn run_git_with_policy(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
) -> AppResult<GitOutput> {
    process::run(root, args, stdout_limit, policy).await
}

pub(crate) async fn run_git_with_authority(
    root: &Path,
    args: &[&str],
    stdout_limit: usize,
    policy: GitProcessPolicy,
    authority: process::GitRootAuthority,
) -> AppResult<GitOutput> {
    process::run_with_authority(root, args, stdout_limit, policy, authority).await
}

pub(crate) fn ensure_success(output: &GitOutput) -> AppResult<()> {
    if output.status.success() {
        return Ok(());
    }
    let message = String::from_utf8_lossy(&output.stderr);
    Err(AppError::Git(message.trim().to_owned()))
}
