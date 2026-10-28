//! AP-B B5: a turn's file changes from the workspace's Git state.
//!
//! Harness `fileChange` events miss edits made by shell commands. Before a
//! prompt is dispatched Sage records a bounded fingerprint of the root's dirty
//! files (`git status`, porcelain v1, plus each listed file's size and mtime).
//! When the turn is terminal the same capture is diffed against that mark:
//! every path that appeared, disappeared, or changed during the turn is a turn
//! change. Both the marks and the computed results are bounded and owned by
//! Rust; the WebView only renders them.

use std::{collections::BTreeMap, path::Path, time::UNIX_EPOCH};

/// Same cap as the WebView's Git changes list.
pub(crate) const MAX_TURN_WORKSPACE_CHANGES: usize = 300;
mod capture;
mod directory;
mod ledger;
mod terminal_capture;
pub(crate) use capture::capture;
pub(crate) use ledger::{
    forget_unprepared, invalidate_active, load, publish_baseline, remember, settle_terminal,
};
pub(crate) use terminal_capture::discard_rejected_staging;
#[cfg(test)]
pub(crate) use terminal_capture::freeze_session_for_test;
pub(crate) use terminal_capture::prepare_terminal;

#[cfg(test)]
pub(crate) fn capture_test_lock() -> std::sync::MutexGuard<'static, ()> {
    static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
    LOCK.lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Fingerprint {
    status: String,
    len: Option<u64>,
    modified_nanos: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub(crate) struct WorkspaceMark {
    root_identity: Option<[u8; 24]>,
    entries: BTreeMap<String, Fingerprint>,
    truncated: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum TurnWorkspaceOperation {
    Created,
    Updated,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TurnWorkspaceChange {
    pub(crate) relative_path: String,
    pub(crate) operation: TurnWorkspaceOperation,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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
fn parse_mark(
    anchor: &directory::Directory,
    stdout: &[u8],
    stdout_truncated: bool,
) -> Option<WorkspaceMark> {
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
        let metadata = anchor.metadata(Path::new(path))?;
        entries.insert(
            path.to_owned(),
            Fingerprint {
                status,
                len: metadata.as_ref().map(std::fs::Metadata::len),
                modified_nanos: metadata
                    .and_then(|metadata| metadata.modified().ok())
                    .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                    .and_then(|elapsed| u64::try_from(elapsed.as_nanos()).ok()),
            },
        );
    }
    Some(WorkspaceMark {
        root_identity: anchor.identity(),
        entries,
        truncated,
    })
}

/// Paths that appeared, disappeared, or changed between two marks.
pub(crate) fn diff_marks(before: &WorkspaceMark, after: &WorkspaceMark) -> TurnWorkspaceChanges {
    if before.root_identity != after.root_identity || before.root_identity.is_none() {
        return TurnWorkspaceChanges::untracked();
    }
    if before.truncated || after.truncated {
        return TurnWorkspaceChanges {
            truncated: true,
            ..TurnWorkspaceChanges::untracked()
        };
    }
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

#[cfg(test)]
#[path = "turn_workspace_tests.rs"]
mod tests;
