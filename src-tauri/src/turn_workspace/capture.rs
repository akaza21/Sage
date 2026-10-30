use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

use super::directory::Directory;
use super::{MAX_TURN_WORKSPACE_CHANGES, WorkspaceMark, history, parse_mark};
use crate::{
    repository_git::{RepositoryBoundary, repository_boundary, run_git},
    workspace::{
        directory_skip::skip_workspace_directory_name,
        file_walk::{MAX_WORKSPACE_FILES, MAX_WORKSPACE_QUEUED_DIRECTORIES},
    },
};

const MAX_INDEX_BYTES: usize = 16 * 1024 * 1024;
const MAX_STATUS_BYTES: usize = 1024 * 1024;
const CAPTURE_DEADLINE: Duration = Duration::from_secs(10);
static SNAPSHOTS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);

pub(crate) async fn capture(root: &Path) -> Option<WorkspaceMark> {
    tokio::time::timeout(
        CAPTURE_DEADLINE,
        capture_with_limit(root, MAX_WORKSPACE_FILES),
    )
    .await
    .ok()
    .flatten()
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
                if mark.entries.contains_key(path) || mark.committed.contains_key(path) {
                    continue;
                }
                let metadata = anchor.metadata(Path::new(path))?;
                if metadata
                    .as_ref()
                    .is_some_and(|metadata| !metadata.is_file())
                {
                    return None;
                }
                mark.terminal_presence
                    .insert(path.clone(), metadata.is_some());
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
    let _permit = SNAPSHOTS.try_acquire().ok()?;
    let deadline = Instant::now() + CAPTURE_DEADLINE;
    let anchor = Directory::root(root)?;
    let identity = anchor.identity()?;
    if root.canonicalize().ok()?.as_path() != root
        || !matches!(
            repository_boundary(root).await.ok()?,
            RepositoryBoundary::Exact
        )
    {
        return None;
    }
    let head = history::read_head(root).await?;
    let index = run_git(root, &["ls-files", "--cached", "-z"], MAX_INDEX_BYTES)
        .await
        .ok()?;
    if !index.succeeded() || index.stdout_truncated {
        return None;
    }
    let mut tracked = BTreeSet::new();
    for (seen, path) in index
        .stdout
        .split(|byte| *byte == 0)
        .filter(|path| !path.is_empty())
        .enumerate()
    {
        if seen as u64 >= limit {
            return None;
        }
        let path = std::str::from_utf8(path).ok()?;
        if !super::excluded_path(path) {
            tracked.insert(path.to_owned());
        }
    }
    let status = run_git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=no",
            "--no-renames",
            "--",
            ".",
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
    let mut pending = vec![(PathBuf::new(), identity)];
    let mut seen = 0;
    while let Some((relative_directory, expected_identity)) = pending.pop() {
        let directory = anchor.child(&relative_directory)?;
        if directory.identity()? != expected_identity {
            return None;
        }
        let mut batch = Vec::with_capacity(128);
        for entry in directory.entries()? {
            let entry = entry.ok()?;
            if skip_workspace_directory_name(&entry.file_name()) {
                continue;
            }
            seen += 1;
            if seen > limit || Instant::now() >= deadline {
                return None;
            }
            let kind = entry.file_type().ok()?;
            if kind.is_symlink() {
                continue;
            }
            let relative_path = relative_directory.join(entry.file_name());
            let relative = relative_path.to_str()?.replace('\\', "/");
            if kind.is_dir() || !tracked.contains(&relative) {
                batch.push((relative, kind.is_dir()));
            }
            if batch.len() == 128 {
                admit_batch(
                    root,
                    &anchor,
                    &directory,
                    &relative_directory,
                    &mut mark,
                    &mut pending,
                    &batch,
                )
                .await?;
                batch.clear();
            }
        }
        admit_batch(
            root,
            &anchor,
            &directory,
            &relative_directory,
            &mut mark,
            &mut pending,
            &batch,
        )
        .await?;
    }
    let current = Directory::root(root)?;
    mark.root_identity = Some(identity);
    (Instant::now() < deadline && current.identity()? == identity).then_some(mark)
}

async fn admit_batch(
    root: &Path,
    anchor: &Directory,
    directory: &Directory,
    relative_directory: &Path,
    mark: &mut WorkspaceMark,
    pending: &mut Vec<(PathBuf, [u8; 24])>,
    batch: &[(String, bool)],
) -> Option<()> {
    if batch.is_empty() {
        return Some(());
    }
    // `-z` is only valid with `--stdin`, which this bounded runner does not
    // feed; paths come back one per line, unquoted with `core.quotePath`
    // off. `check-ignore` rejects the runner's literal pathspec magic, so it
    // is switched off here and names that could read as magic (a leading
    // `:`) or break the line protocol are not asked about: they stay visible.
    let mut args = vec![
        "--no-literal-pathspecs",
        "-c",
        "core.quotePath=false",
        "check-ignore",
        "--no-index",
        "--",
    ];
    args.extend(
        batch
            .iter()
            .map(|entry| entry.0.as_str())
            .filter(|path| !path.contains(['\n', '\r']) && !path.starts_with(':')),
    );
    let output = if args.len() > 6 {
        let output = run_git(root, &args, MAX_STATUS_BYTES).await.ok()?;
        if output.stdout_truncated || !matches!(output.exit_code(), Some(0 | 1)) {
            return None;
        }
        output.stdout
    } else {
        Vec::new()
    };
    let ignored: BTreeSet<_> = output
        .split(|byte| *byte == b'\n')
        .map(|line| line.strip_suffix(b"\r").unwrap_or(line))
        .collect();
    if anchor.child(relative_directory)?.identity()? != directory.identity()? {
        return None;
    }
    for (relative, directory) in batch {
        if ignored.contains(relative.as_bytes()) {
            continue;
        }
        if *directory {
            if pending.len() == MAX_WORKSPACE_QUEUED_DIRECTORIES {
                return None;
            }
            let child = anchor.child(Path::new(relative))?;
            pending.push((PathBuf::from(relative), child.identity()?));
        } else if mark.entries.len() < MAX_TURN_WORKSPACE_CHANGES {
            let record = format!("?? {relative}\0");
            mark.entries
                .extend(parse_mark(anchor, record.as_bytes(), false)?.entries);
        } else {
            mark.truncated = true;
        }
    }
    (anchor.child(relative_directory)?.identity()? == directory.identity()?).then_some(())
}
