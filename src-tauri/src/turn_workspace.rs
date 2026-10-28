//! AP-B B5: a turn's file changes from the workspace's Git state.
//!
//! Harness `fileChange` events miss edits made by shell commands. Before a
//! prompt is dispatched Sage records a bounded fingerprint of the root's dirty
//! files (`git status`, porcelain v1, plus each listed file's size and mtime).
//! When the turn is terminal the same capture is diffed against that mark:
//! every path that appeared, disappeared, or changed during the turn is a turn
//! change. Both the marks and the computed results are bounded and owned by
//! Rust; the WebView only renders them.

use std::{
    collections::{BTreeMap, VecDeque},
    path::Path,
    sync::Mutex,
    time::UNIX_EPOCH,
};

/// Same cap as the WebView's Git changes list.
pub(crate) const MAX_TURN_WORKSPACE_CHANGES: usize = 300;
const MAX_STATUS_BYTES: usize = 1024 * 1024;
const MAX_TRACKED_TURNS: usize = 64;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    status: String,
    len: Option<u64>,
    modified_nanos: Option<u128>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMark {
    entries: BTreeMap<String, Fingerprint>,
    truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TurnWorkspaceOperation {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnWorkspaceChange {
    pub(crate) relative_path: String,
    pub(crate) operation: TurnWorkspaceOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnWorkspaceChanges {
    pub(crate) tracked: bool,
    pub(crate) truncated: bool,
    pub(crate) changes: Vec<TurnWorkspaceChange>,
}

impl TurnWorkspaceChanges {
    pub(crate) fn untracked() -> Self {
        Self {
            tracked: false,
            truncated: false,
            changes: Vec::new(),
        }
    }
}

/// Parses `git status --porcelain=v1 -z --no-renames` into a bounded mark.
pub(crate) fn parse_mark(root: &Path, stdout: &[u8], stdout_truncated: bool) -> WorkspaceMark {
    let mut entries = BTreeMap::new();
    let mut truncated = stdout_truncated;
    for record in stdout.split(|byte| *byte == 0) {
        if record.len() < 4 || record[2] != b' ' {
            continue;
        }
        let Ok(path) = std::str::from_utf8(&record[3..]) else {
            continue;
        };
        if entries.len() == MAX_TURN_WORKSPACE_CHANGES {
            truncated = true;
            break;
        }
        let status = String::from_utf8_lossy(&record[..2]).into_owned();
        let metadata = std::fs::symlink_metadata(root.join(path)).ok();
        entries.insert(
            path.to_owned(),
            Fingerprint {
                status,
                len: metadata.as_ref().map(std::fs::Metadata::len),
                modified_nanos: metadata
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .map(|elapsed| elapsed.as_nanos()),
            },
        );
    }
    WorkspaceMark { entries, truncated }
}

/// Paths that appeared, disappeared, or changed between two marks.
pub(crate) fn diff_marks(before: &WorkspaceMark, after: &WorkspaceMark) -> TurnWorkspaceChanges {
    let mut changes = Vec::new();
    let mut truncated = before.truncated || after.truncated;
    let mut push = |relative_path: &str, operation| {
        if changes.len() == MAX_TURN_WORKSPACE_CHANGES {
            truncated = true;
        } else {
            changes.push(TurnWorkspaceChange {
                relative_path: relative_path.to_owned(),
                operation,
            });
        }
    };
    for (path, now) in &after.entries {
        if before.entries.get(path) == Some(now) {
            continue;
        }
        let operation = if now.len.is_none() || now.status.contains('D') {
            TurnWorkspaceOperation::Deleted
        } else if now.status == "??" || now.status.starts_with('A') {
            TurnWorkspaceOperation::Created
        } else {
            TurnWorkspaceOperation::Updated
        };
        push(path, operation);
    }
    for path in before.entries.keys() {
        if !after.entries.contains_key(path) {
            // Dirty before, clean now: the turn reverted or committed it.
            push(path, TurnWorkspaceOperation::Updated);
        }
    }
    TurnWorkspaceChanges {
        tracked: true,
        truncated,
        changes,
    }
}

/// Captures the root's dirty-file mark. Only an exact repository root is
/// supported, matching the Source Control view.
pub(crate) async fn capture(root: &Path) -> Option<WorkspaceMark> {
    if !matches!(
        crate::repository_git::repository_boundary(root)
            .await
            .ok()?,
        crate::repository_git::RepositoryBoundary::Exact
    ) {
        return None;
    }
    let output = crate::repository_git::run_git(
        root,
        &[
            "status",
            "--porcelain=v1",
            "-z",
            "--untracked-files=all",
            "--no-renames",
        ],
        MAX_STATUS_BYTES,
    )
    .await
    .ok()?;
    crate::repository_git::ensure_success(&output).ok()?;
    Some(parse_mark(root, &output.stdout, output.stdout_truncated))
}

enum Entry {
    Marked(WorkspaceMark),
    Settled(TurnWorkspaceChanges),
}

/// Bounded ledger keyed by the turn's client intent identity.
static LEDGER: Mutex<VecDeque<(String, Entry)>> = Mutex::new(VecDeque::new());

fn ledger() -> std::sync::MutexGuard<'static, VecDeque<(String, Entry)>> {
    LEDGER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

pub(crate) fn remember(key: String, mark: WorkspaceMark) {
    let mut ledger = ledger();
    ledger.retain(|(existing, _)| existing != &key);
    if ledger.len() == MAX_TRACKED_TURNS {
        ledger.pop_front();
    }
    ledger.push_back((key, Entry::Marked(mark)));
}

/// Diffs a terminal turn once against the current mark, then keeps the
/// result so later reads are stable.
pub(crate) fn settle(key: &str, now: Option<WorkspaceMark>) -> TurnWorkspaceChanges {
    let mut ledger = ledger();
    let Some((_, entry)) = ledger.iter_mut().find(|(existing, _)| existing == key) else {
        return TurnWorkspaceChanges::untracked();
    };
    match entry {
        Entry::Settled(changes) => changes.clone(),
        Entry::Marked(before) => {
            let Some(now) = now else {
                return TurnWorkspaceChanges::untracked();
            };
            let changes = diff_marks(before, &now);
            *entry = Entry::Settled(changes.clone());
            changes
        }
    }
}

pub(crate) fn is_settled(key: &str) -> Option<bool> {
    ledger()
        .iter()
        .find(|(existing, _)| existing == key)
        .map(|(_, entry)| matches!(entry, Entry::Settled(_)))
}

#[cfg(test)]
#[path = "turn_workspace_tests.rs"]
mod tests;
