//! Where Git found the repository: the `.git` entries on the discovery path
//! and the locations `rev-parse` reports.

use std::path::{Path, PathBuf};

use sage_core::error::{AppError, AppResult};

use super::{Fingerprint, GitOutput, paths};

/// Keeps the `.git` entries from the root up to the one Git used. The verdict
/// is cacheable only when every entry below that one is missing, because an
/// existing entry Git skipped could later become the repository it opens.
pub(super) fn discovery_path(
    ancestors: Vec<(PathBuf, Fingerprint)>,
    git_dir: &Path,
) -> (Vec<(PathBuf, Fingerprint)>, bool) {
    let mut watched = Vec::new();
    for (entry, fingerprint) in ancestors {
        let missing = fingerprint == Fingerprint::Missing;
        let used = !missing && resolves_to(&entry, &fingerprint, git_dir);
        watched.push((entry, fingerprint));
        if used {
            return (watched, true);
        }
        if !missing {
            return (watched, false);
        }
    }
    // Git opened a repository that no `.git` entry on the path names.
    (watched, false)
}

fn resolves_to(entry: &Path, fingerprint: &Fingerprint, git_dir: &Path) -> bool {
    let target = match fingerprint {
        Fingerprint::Directory(_) => entry.to_path_buf(),
        Fingerprint::Contents { .. } => match gitfile_target(entry) {
            Some(target) => target,
            None => return false,
        },
        _ => return false,
    };
    match (target.canonicalize(), git_dir.canonicalize()) {
        (Ok(target), Ok(git_dir)) => target == git_dir,
        _ => false,
    }
}

/// The directory the `gitdir:` file at `entry` names, read as Git reads it.
/// It only decides whether a verdict may be cached, and any doubt means it
/// may not; the layout itself is checked in [`super::layout`].
fn gitfile_target(entry: &Path) -> Option<PathBuf> {
    let bytes = std::fs::read(entry).ok()?;
    paths::written_prefixes(entry.parent()?, paths::gitfile_path(&bytes)?)?.pop()
}
pub(super) struct Location {
    pub(super) git_dir: PathBuf,
    pub(super) common_dir: PathBuf,
    pub(super) work_tree: PathBuf,
}

impl Location {
    pub(super) fn parse(output: &GitOutput) -> AppResult<Self> {
        let text = std::str::from_utf8(&output.stdout)
            .map_err(|_| AppError::Git("Git repository path is not valid UTF-8".into()))?;
        let mut lines = text
            .lines()
            .filter(|line| !line.is_empty())
            .map(PathBuf::from);
        match (lines.next(), lines.next(), lines.next()) {
            (Some(git_dir), Some(common_dir), Some(work_tree)) => Ok(Self {
                git_dir,
                common_dir,
                work_tree,
            }),
            _ => Err(AppError::Git(
                "Git did not report the repository location".into(),
            )),
        }
    }
}
