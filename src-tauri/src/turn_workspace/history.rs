//! Commits made during a turn. A shell command can edit a clean tracked file
//! and commit it before the turn ends; both `git status` captures are then
//! clean. The HEAD recorded at turn start is diffed against the HEAD at turn
//! end so those paths are still turn changes, bounded like the dirty set.

use std::{collections::BTreeMap, path::Path};

use super::{MAX_TURN_WORKSPACE_CHANGES, TurnWorkspaceOperation};
use crate::repository_git::run_git;

const MAX_HISTORY_BYTES: usize = 1024 * 1024;

/// HEAD as recorded in a mark. Marks stored before HEAD was recorded have no
/// `HeadMark`, and are never diffed against history.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HeadMark {
    Unborn,
    Commit(String),
}

pub(super) async fn read_head(root: &Path) -> Option<HeadMark> {
    let output = run_git(root, &["rev-parse", "--verify", "-q", "HEAD"], 256)
        .await
        .ok()?;
    if output.stdout_truncated {
        return None;
    }
    match output.exit_code() {
        Some(0) => {
            let oid = std::str::from_utf8(&output.stdout).ok()?.trim();
            is_object_id(oid).then(|| HeadMark::Commit(oid.to_owned()))
        }
        Some(1) => Some(HeadMark::Unborn),
        _ => None,
    }
}

/// Committed paths between `before` and `after`. `None` when history cannot
/// be read; the bool is true when the bounded set was truncated.
pub(super) async fn committed_between(
    root: &Path,
    before: &HeadMark,
    after: &HeadMark,
) -> Option<(BTreeMap<String, TurnWorkspaceOperation>, bool)> {
    let after = match after {
        HeadMark::Commit(oid) => oid.as_str(),
        HeadMark::Unborn => return Some((BTreeMap::new(), false)),
    };
    let before = match before {
        HeadMark::Commit(oid) if oid == after => return Some((BTreeMap::new(), false)),
        HeadMark::Commit(oid) => oid.clone(),
        HeadMark::Unborn => empty_tree(root).await?,
    };
    let before = before.as_str();
    let output = run_git(
        root,
        &[
            "diff",
            "--name-status",
            "-z",
            "--no-renames",
            "--no-ext-diff",
            before,
            after,
            "--",
            ".",
            ":(exclude,glob,icase)**/node_modules/**",
        ],
        MAX_HISTORY_BYTES,
    )
    .await
    .ok()?;
    if !output.succeeded() {
        return None;
    }
    let (paths, truncated) = parse_name_status(&output.stdout);
    Some((paths, truncated || output.stdout_truncated))
}

/// The empty tree in the repository's object format (an unborn HEAD).
async fn empty_tree(root: &Path) -> Option<String> {
    let output = run_git(root, &["hash-object", "-t", "tree", "--stdin"], 256)
        .await
        .ok()?;
    if !output.succeeded() || output.stdout_truncated {
        return None;
    }
    let oid = std::str::from_utf8(&output.stdout).ok()?.trim();
    is_object_id(oid).then(|| oid.to_owned())
}

fn is_object_id(value: &str) -> bool {
    matches!(value.len(), 40 | 64) && value.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// Parses `git diff --name-status -z` records into a bounded path set.
pub(super) fn parse_name_status(stdout: &[u8]) -> (BTreeMap<String, TurnWorkspaceOperation>, bool) {
    let mut paths = BTreeMap::new();
    let mut fields = stdout.split(|byte| *byte == 0);
    while let (Some(status), Some(path)) = (fields.next(), fields.next()) {
        let Ok(path) = std::str::from_utf8(path) else {
            continue;
        };
        if path.is_empty() {
            continue;
        }
        if paths.len() == MAX_TURN_WORKSPACE_CHANGES {
            return (paths, true);
        }
        let operation = match status.first() {
            Some(b'A') => TurnWorkspaceOperation::Created,
            Some(b'D') => TurnWorkspaceOperation::Deleted,
            _ => TurnWorkspaceOperation::Updated,
        };
        paths.insert(path.to_owned(), operation);
    }
    (paths, false)
}
