//! Index membership for ignored notices. One bounded Git read per ignored
//! subtree and index stamp; steady build output starts no Git processes.

use std::{
    collections::{BTreeSet, HashMap},
    path::Path,
};

use crate::{
    repository_change_stamp::index_stamp,
    repository_git::{RepositoryBoundary, repository_boundary, run_git},
    workspace::file_walk::MAX_WORKSPACE_FILES,
};

const MAX_TRACKED_PATH_BYTES: usize = 16 * 1024 * 1024;
const MAX_TRACKED_FOLDERS: usize = 64;

#[derive(Default)]
pub(super) struct TrackedPaths {
    stamp: Option<u64>,
    // None means unavailable or over budget: keep Git notifications rather
    // than silently losing tracked edits because a partial set omitted them.
    folders: HashMap<String, Option<BTreeSet<Vec<u8>>>>,
    path_bytes: usize,
    path_count: usize,
}

impl TrackedPaths {
    pub(super) fn relevant(
        &mut self,
        root: &Path,
        folder: &str,
        relative: &str,
        moved: bool,
    ) -> bool {
        let stamp = index_stamp(root);
        if self.stamp != Some(stamp) {
            self.folders.clear();
            self.path_bytes = 0;
            self.path_count = 0;
            self.stamp = Some(stamp);
        }
        if !self.folders.contains_key(folder) {
            if self.folders.len() >= MAX_TRACKED_FOLDERS {
                return true;
            }
            let loaded = tauri::async_runtime::block_on(load(
                root,
                folder,
                MAX_TRACKED_PATH_BYTES - self.path_bytes,
            ));
            // If the index changed during the read, notify conservatively
            // and reload next time; this set might describe either version.
            if index_stamp(root) != stamp {
                self.stamp = None;
                return true;
            }
            let paths = loaded
                .filter(|(paths, _)| self.path_count + paths.len() <= MAX_WORKSPACE_FILES as usize)
                .map(|(paths, bytes)| {
                    self.path_bytes += bytes;
                    self.path_count += paths.len();
                    paths
                });
            self.folders.insert(folder.to_owned(), paths);
        }
        let Some(Some(paths)) = self.folders.get(folder) else {
            return true;
        };
        let mut path = key(relative.as_bytes());
        if paths.contains(&path) {
            return true;
        }
        // Removing or renaming a directory can remove tracked descendants.
        // A directory's ordinary changed record only reports entry churn.
        if !moved {
            return false;
        }
        path.push(b'/');
        paths
            .range(path.clone()..)
            .next()
            .is_some_and(|tracked| tracked.starts_with(&path))
    }
}

async fn load(root: &Path, folder: &str, byte_limit: usize) -> Option<(BTreeSet<Vec<u8>>, usize)> {
    match repository_boundary(root).await.ok()? {
        RepositoryBoundary::Exact => {}
        RepositoryBoundary::Nested | RepositoryBoundary::NotRepository => {
            return Some((BTreeSet::new(), 0));
        }
    }
    // Windows can report a case-only rename that the index still spells the
    // old way. Override the runner's global literal mode with an explicitly
    // literal, case-insensitive pathspec; metacharacters remain file names.
    let pathspec = if cfg!(windows) {
        format!(":(icase,literal){folder}")
    } else {
        folder.to_string()
    };
    let mut args = vec!["ls-files", "--cached", "-z", "--", pathspec.as_str()];
    if cfg!(windows) {
        args.insert(0, "--no-literal-pathspecs");
    }
    let output = run_git(root, &args, byte_limit).await.ok()?;
    if !output.succeeded() || output.stdout_truncated {
        return None;
    }
    Some((parse(&output.stdout)?, output.stdout.len()))
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
mod tests;
