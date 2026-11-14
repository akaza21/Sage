//! Which repository layouts admission accepts, and every file each relies on.
//!
//! Three layouts are admitted, all with the opened root as the work tree:
//! - the root's own `.git` directory, as both git dir and common dir;
//! - a separate git dir: `.git` is a `gitdir:` file naming a git dir with no
//!   `commondir` file;
//! - a linked worktree: `.git` names a git dir whose `commondir` file names
//!   the shared common dir.
//!
//! Each `gitdir:` and `commondir` file is read exactly as Git reads it, and
//! the path it names is joined to the file's directory as written and
//! resolved by the operating system, through any symlink, junction or `..`,
//! just as Git resolves it. The directory reached is what Git opens, so that
//! is what is inspected: its config goes through the same policy. Every
//! directory on the way is stamped with the identity of what it reaches, so a
//! later retarget invalidates the cached verdict. The layout Git reports
//! (`rev-parse`) must be the one resolved here.

use std::path::{Component, Path, PathBuf};

use sage_core::error::{AppResult, RepositoryLayout};

use super::{
    Fingerprint, GIT_DIR_NAME, Location, POINTER_FILES, fingerprint::watch_reading, paths, watch,
};
use crate::process::parallel_limits::ParallelLimits;

/// The repository admission inspected. Admitted Git calls run with these
/// explicit locations, so they open exactly this repository.
///
/// `git_dir` and `work_tree` are relative to the root: the child's working
/// directory is the identity-checked root handle, so a directory swapped in
/// at the root's name, or a move of the root, cannot redirect them. For a
/// separate git dir or a linked worktree, `git_dir` is the root's `.git`
/// file, which Git follows to the data admission inspected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::process) struct AdmittedRepository {
    pub(super) git_dir: PathBuf,
    pub(super) work_tree: PathBuf,
    /// The effective values of the settings that start parallel children,
    /// once admission has read them. Until then (and for the admission
    /// probes themselves) nothing is pinned, so the probes read the
    /// repository's own values.
    pub(super) parallel: Option<ParallelLimits>,
}

impl AdmittedRepository {
    pub(in crate::process) fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub(in crate::process) fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    pub(in crate::process) fn parallel_overrides(&self) -> impl Iterator<Item = String> {
        self.parallel
            .into_iter()
            .flat_map(ParallelLimits::overrides)
    }
}

/// An admitted layout: the repository, everything it was decided from, and
/// the config files Git reads for it.
pub(super) struct Layout {
    pub(super) repository: AdmittedRepository,
    pub(super) watched: Vec<(PathBuf, Fingerprint)>,
    pub(super) config_files: Vec<PathBuf>,
}

pub(super) fn resolve(
    root: &Path,
    location: &Location,
) -> AppResult<Result<Layout, RepositoryLayout>> {
    let root = root.canonicalize()?;
    if location.work_tree.canonicalize()? != root {
        return Ok(Err(RepositoryLayout::NestedRoot));
    }
    let reported = Reported {
        git_dir: location.git_dir.canonicalize()?,
        common_dir: location.common_dir.canonicalize()?,
    };
    let mut watched = Vec::new();
    let entry = root.join(GIT_DIR_NAME);
    let ((path, fingerprint), bytes) = watch_reading(entry.clone());
    watched.push((path, fingerprint.clone()));
    let admitted = match (fingerprint, bytes) {
        (Fingerprint::Directory(_), _) => own_directory(&entry, &reported, &mut watched),
        (Fingerprint::Contents { .. }, Some(bytes)) => {
            gitfile(&root, &bytes, &reported, &mut watched)
        }
        _ => None,
    };
    Ok(match admitted {
        Some(config_files) => Ok(Layout {
            repository: AdmittedRepository {
                git_dir: PathBuf::from(GIT_DIR_NAME),
                work_tree: PathBuf::from(Component::CurDir.as_os_str()),
                parallel: None,
            },
            watched,
            config_files,
        }),
        None => Err(RepositoryLayout::ExternalGitDir),
    })
}

/// What `rev-parse` reported, canonicalized.
struct Reported {
    git_dir: PathBuf,
    common_dir: PathBuf,
}

/// The config files Git reads for an admitted layout.
type Admitted = Vec<PathBuf>;

fn own_directory(
    entry: &Path,
    reported: &Reported,
    watched: &mut Vec<(PathBuf, Fingerprint)>,
) -> Option<Admitted> {
    let git_dir = entry.canonicalize().ok()?;
    (reported.git_dir == git_dir && reported.common_dir == git_dir).then_some(())?;
    // A `commondir` or `gitdir` file in a `.git` directory belongs to no
    // layout Git creates; it is refused as ambiguous.
    for name in POINTER_FILES {
        let pointer = watch(entry.join(name));
        let missing = pointer.1 == Fingerprint::Missing;
        watched.push(pointer);
        missing.then_some(())?;
    }
    Some(configs(entry, entry))
}

fn gitfile(
    root: &Path,
    bytes: &[u8],
    reported: &Reported,
    watched: &mut Vec<(PathBuf, Fingerprint)>,
) -> Option<Admitted> {
    let written = paths::gitfile_path(bytes)?;
    let git_dir = walk(root, written, &mut |path| push_watch(watched, path))?;
    (reported.git_dir == git_dir).then_some(())?;
    let ((path, fingerprint), commondir) = watch_reading(git_dir.join("commondir"));
    watched.push((path, fingerprint.clone()));
    let common_dir = match (fingerprint, commondir) {
        // A separate git dir.
        (Fingerprint::Missing, _) => git_dir.clone(),
        // A linked worktree's git dir, sharing `<common>`.
        (Fingerprint::Contents { .. }, Some(commondir)) => {
            walk(&git_dir, paths::commondir_path(&commondir)?, &mut |path| {
                push_watch(watched, path)
            })?
        }
        _ => return None,
    };
    (reported.common_dir == common_dir).then_some(())?;
    Some(configs(&common_dir, &git_dir))
}

fn push_watch(watched: &mut Vec<(PathBuf, Fingerprint)>, path: PathBuf) -> Fingerprint {
    let watched_entry = watch(path);
    let fingerprint = watched_entry.1.clone();
    watched.push(watched_entry);
    fingerprint
}

/// Walks `written` from `base` the way the operating system (and so Git)
/// resolves it, stamping every directory on the way with `watch`, and returns
/// the canonical directory it reaches.
fn walk(
    base: &Path,
    written: &str,
    watch: &mut dyn FnMut(PathBuf) -> Fingerprint,
) -> Option<PathBuf> {
    let prefixes = paths::written_prefixes(base, written)?;
    for prefix in &prefixes {
        if !watch(prefix.clone()).is_directory() {
            return None;
        }
    }
    prefixes.last()?.canonicalize().ok()
}

/// The config Git reads from `common`, and the per-worktree config from
/// `git_dir` (the same directory outside a linked worktree).
fn configs(common: &Path, git_dir: &Path) -> Vec<PathBuf> {
    let mut configs = vec![common.join("config"), common.join("config.worktree")];
    if git_dir != common {
        configs.push(git_dir.join("config.worktree"));
    }
    configs
}
