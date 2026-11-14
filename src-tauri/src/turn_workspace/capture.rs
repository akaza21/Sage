use std::{
    path::Path,
    time::{Duration, Instant},
};

use super::directory::Directory;
use super::{MAX_TURN_WORKSPACE_CHANGES, WorkspaceMark, history, parse_mark};
use crate::{
    repository_git::{RepositoryBoundary, repository_boundary, run_git},
    workspace::file_walk::MAX_WORKSPACE_FILES,
};

const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_STATUS_BYTES: usize = 1024 * 1024;
// Optional evidence must leave send and journal capacity available promptly.
const CAPTURE_DEADLINE: Duration = Duration::from_millis(super::CAPTURE_BUDGET_MILLIS);
static SNAPSHOTS: tokio::sync::Semaphore =
    tokio::sync::Semaphore::const_new(super::MAX_CONCURRENT_CAPTURES);

pub(crate) enum BaselineCapture {
    Captured(Box<WorkspaceMark>),
    Unsupported,
    Failed,
}

pub(crate) async fn capture_baseline(root: &Path) -> BaselineCapture {
    if Directory::root(root).is_none() {
        return BaselineCapture::Failed;
    }
    tokio::time::timeout(CAPTURE_DEADLINE, async {
        match repository_boundary(root).await {
            Ok(RepositoryBoundary::Exact) => capture_known_repository(root, MAX_WORKSPACE_FILES)
                .await
                .map_or(BaselineCapture::Failed, |mark| {
                    BaselineCapture::Captured(Box::new(mark))
                }),
            // Existing Git metadata that Git cannot admit is a capture failure.
            Ok(RepositoryBoundary::NotRepository)
                if root.join(".git").try_exists().unwrap_or(true) =>
            {
                BaselineCapture::Failed
            }
            Ok(_) => BaselineCapture::Unsupported,
            Err(_) => BaselineCapture::Failed,
        }
    })
    .await
    .unwrap_or(BaselineCapture::Failed)
}

#[cfg(test)]
pub(crate) async fn capture(root: &Path) -> Option<WorkspaceMark> {
    match capture_baseline(root).await {
        BaselineCapture::Captured(mark) => Some(*mark),
        _ => None,
    }
}

/// The terminal capture: a mark plus the paths committed since `before`'s
/// HEAD, bounded like the dirty set (overflow marks the result truncated).
pub(crate) async fn capture_after(root: &Path, before: &WorkspaceMark) -> Option<WorkspaceMark> {
    tokio::time::timeout(CAPTURE_DEADLINE, async {
        let deadline = Instant::now() + CAPTURE_DEADLINE;
        if before.entries.len() > MAX_TURN_WORKSPACE_CHANGES {
            return None;
        }
        let anchor = Directory::root(root)?;
        let identity = anchor.identity()?;
        if before.root_identity != Some(identity) {
            return None;
        }
        let mut mark = capture_with_limit(root, MAX_WORKSPACE_FILES).await?;
        if mark.root_identity != Some(identity) {
            return None;
        }
        if let (Some(before_head), Some(after_head)) = (&before.head, &mark.head) {
            let (committed, truncated) =
                history::committed_between(root, before_head, after_head).await?;
            mark.committed = committed;
            mark.truncated |= truncated;
        }
        if !before.truncated && !mark.truncated {
            for path in before.entries.keys() {
                if Instant::now() >= deadline {
                    return None;
                }
                // Git can still report a baseline path after a symlink substitution.
                // Validate it before dirty or committed membership skips presence capture.
                let entry = anchor.entry_status(Path::new(path))?;
                if entry.is_some_and(|entry| !entry.regular_file) {
                    return None;
                }
                if mark.entries.contains_key(path) || mark.committed.contains_key(path) {
                    continue;
                }
                mark.terminal_presence.insert(path.clone(), entry.is_some());
            }
        }
        (Instant::now() < deadline && Directory::root(root)?.identity()? == identity)
            .then_some(mark)
    })
    .await
    .ok()
    .flatten()
}

pub(super) async fn capture_with_limit(root: &Path, limit: u64) -> Option<WorkspaceMark> {
    if !matches!(
        repository_boundary(root).await.ok()?,
        RepositoryBoundary::Exact
    ) {
        return None;
    }
    capture_known_repository(root, limit).await
}

async fn capture_known_repository(root: &Path, limit: u64) -> Option<WorkspaceMark> {
    let _permit = SNAPSHOTS.try_acquire().ok()?;
    let deadline = Instant::now() + CAPTURE_DEADLINE;
    let anchor = Directory::root(root)?;
    let identity = anchor.identity()?;
    if root.canonicalize().ok()?.as_path() != root {
        return None;
    }
    // HEAD and the size check are independent reads of the same repository,
    // so they run side by side. The status walk waits for the size check, so a
    // repository too large to mark never pays for it, and a capture never holds
    // more than two of the git pool's processes. At the capture limit that can
    // briefly fill the pool, but only for the length of a `rev-parse`.
    let (head, index) = tokio::join!(
        history::read_head(root),
        run_git(
            root,
            &[
                "--no-literal-pathspecs",
                "ls-files",
                "--cached",
                "--others",
                "--exclude-standard",
                "-z",
                "--",
                ".",
                ":(exclude,icase)node_modules/**",
                ":(exclude,icase)**/node_modules/**",
                ":(exclude,icase).git/**",
                ":(exclude,icase)**/.git/**",
            ],
            MAX_INDEX_BYTES,
        ),
    );
    let head = head?;
    let index = index.ok()?;
    if !index.succeeded() || index.stdout_truncated {
        return None;
    }
    for (seen, path) in index
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .enumerate()
    {
        if seen as u64 >= limit {
            return None;
        }
        std::str::from_utf8(path).ok()?;
    }
    let status = run_git(
        root,
        &[
            "--no-literal-pathspecs",
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
            "--",
            ".",
            ":(exclude,icase)node_modules/**",
            ":(exclude,icase)**/node_modules/**",
            ":(exclude,icase).git/**",
            ":(exclude,icase)**/.git/**",
        ],
        MAX_STATUS_BYTES,
    )
    .await
    .ok()?;
    if !status.succeeded() || status.stdout_truncated {
        return None;
    }
    let mut mark = parse_mark(&anchor, &status.stdout, false)?;
    mark.head = Some(head);
    let current = Directory::root(root)?;
    mark.root_identity = Some(identity);
    (Instant::now() < deadline && current.identity()? == identity).then_some(mark)
}
