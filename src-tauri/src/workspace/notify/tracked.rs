//! Bounded index lookup for one window. Git reads a fresh index, so force-adds
//! and index removals need no watcher-owned membership/fingerprint cache.
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, atomic::AtomicBool},
};

use crate::{error::AppError, git_process, workspace::file_walk::MAX_WORKSPACE_FILES};

const MAX_TRACKED_PATH_BYTES: usize = 16 * 1024 * 1024;

pub(super) async fn all_untracked(
    root: &Path,
    paths: &BTreeMap<String, bool>,
    cancelled: Arc<AtomicBool>,
) -> Result<bool, AppError> {
    // Limit the lookup to the common containing folder. Unrelated index
    // entries cannot exhaust an ignored build-output subtree's budget.
    let mut folder = paths
        .keys()
        .next()
        .and_then(|path| path.rsplit_once('/'))
        .map_or("", |(parent, _)| parent);
    for path in paths.keys() {
        while !folder.is_empty()
            && !path
                .strip_prefix(folder)
                .is_some_and(|rest| rest.starts_with('/'))
        {
            folder = folder.rsplit_once('/').map_or("", |(parent, _)| parent);
        }
    }
    let pathspec = if cfg!(windows) {
        format!(":(icase,literal){folder}")
    } else {
        folder.to_string()
    };
    let mut args = vec!["ls-files", "--cached", "-z"];
    if !folder.is_empty() {
        args.extend(["--", pathspec.as_str()]);
        if cfg!(windows) {
            args.insert(0, "--no-literal-pathspecs");
        }
    }
    let output =
        git_process::run_cancellable(root, &args, MAX_TRACKED_PATH_BYTES, None, cancelled).await?;
    if !output.status.success() || output.stdout_truncated {
        return Ok(false);
    }
    let Some(tracked) = parse(&output.stdout) else {
        return Ok(false);
    };
    Ok(paths.iter().all(|(path, moved)| {
        let mut path_key = key(path.trim_end_matches('/').as_bytes());
        if tracked.contains(&path_key) {
            return false;
        }
        if !moved {
            return true;
        }
        path_key.push(b'/');
        !tracked
            .range(path_key.clone()..)
            .next()
            .is_some_and(|path| path.starts_with(&path_key))
    }))
}

fn parse(bytes: &[u8]) -> Option<BTreeSet<Vec<u8>>> {
    if bytes.len() > MAX_TRACKED_PATH_BYTES {
        return None;
    }
    let mut paths = BTreeSet::new();
    if bytes.is_empty() {
        return Some(paths);
    }
    for (index, path) in bytes
        .strip_suffix(&[0])?
        .split(|byte| *byte == 0)
        .enumerate()
    {
        if index as u64 >= MAX_WORKSPACE_FILES || path.is_empty() {
            return None;
        }
        paths.insert(key(path));
    }
    Some(paths)
}

fn key(path: &[u8]) -> Vec<u8> {
    #[cfg(windows)]
    {
        path.to_ascii_lowercase()
    }
    #[cfg(not(windows))]
    {
        path.to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn incomplete_or_over_budget_indexes_never_supply_partial_membership() {
        assert_eq!(parse(b"dist/a.js\0dist/b.js\0").unwrap().len(), 2);
        assert!(parse(b"dist/a.js\0dist/b.js").is_none());
        assert!(parse(&b"a\0".repeat(MAX_WORKSPACE_FILES as usize + 1)).is_none());
    }
}
