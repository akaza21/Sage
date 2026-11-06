//! Which repository layouts admission accepts, and every file each relies on.
//!
//! Three layouts are admitted, all with the opened root as the work tree:
//! - the root's own `.git` directory, as both git dir and common dir;
//! - a separate git dir: `.git` is a `gitdir:` file naming a directory that
//!   holds no `commondir` or `gitdir` pointer of its own;
//! - a linked worktree: `.git` names `<common>/worktrees/<name>`, whose
//!   `commondir` file names `<common>`, an ordinary git dir.
//!
//! Each `gitdir:` and `commondir` file is read exactly as Git reads it. Every
//! directory on the paths they name is fingerprinted and must be real, never
//! a symlink or junction, so the operating system resolves exactly the path
//! inspected here and a later retarget invalidates the cached verdict. The
//! layout Git reports (`rev-parse`) must be the one read here. Anything else,
//! or anything ambiguous, is refused.

use std::path::{Component, Path, PathBuf};

use super::{
    Fingerprint, GIT_DIR_NAME, Location, POINTER_FILES, fingerprint::watch_reading, paths, watch,
};
use crate::{
    error::{AppResult, RepositoryLayout},
    git_process::parallel_limits::ParallelLimits,
};

/// The repository admission inspected. Admitted Git calls run with these
/// explicit locations, so they open exactly this repository.
///
/// `git_dir` and `work_tree` are relative to the root: the child's working
/// directory is the identity-checked root handle, so a directory swapped in
/// at the root's name, or a move of the root, cannot redirect them. For a
/// separate git dir or a linked worktree, `git_dir` is the root's `.git`
/// file, which Git follows to the data admission checked.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(in crate::git_process) struct AdmittedRepository {
    pub(super) git_dir: PathBuf,
    pub(super) work_tree: PathBuf,
    /// The repository's git data when it lies outside the root.
    pub(super) data_dir: Option<PathBuf>,
    /// The effective values of the settings that start parallel children,
    /// once admission has read them. Until then (and for the admission
    /// probes themselves) nothing is pinned, so the probes read the
    /// repository's own values.
    pub(super) parallel: Option<ParallelLimits>,
}

impl AdmittedRepository {
    /// A submodule's repository, at root-relative locations.
    pub(super) fn submodule(git_dir: PathBuf, work_tree: PathBuf) -> Self {
        Self {
            git_dir,
            work_tree,
            data_dir: None,
            parallel: None,
        }
    }

    pub(in crate::git_process) fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub(in crate::git_process) fn work_tree(&self) -> &Path {
        &self.work_tree
    }

    /// Where the repository's git data (its index, say) lives.
    pub(super) fn data_dir(&self, root: &Path) -> PathBuf {
        self.data_dir
            .clone()
            .unwrap_or_else(|| root.join(&self.git_dir))
    }

    pub(in crate::git_process) fn parallel_overrides(&self) -> impl Iterator<Item = String> {
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
        (Fingerprint::Directory, _) => own_directory(&entry, &reported, &mut watched),
        (Fingerprint::Contents { .. }, Some(bytes)) => {
            gitfile(&root, &bytes, &reported, &mut watched)
        }
        _ => None,
    };
    Ok(match admitted {
        Some((data_dir, config_files)) => Ok(Layout {
            repository: AdmittedRepository {
                git_dir: PathBuf::from(GIT_DIR_NAME),
                work_tree: PathBuf::from(Component::CurDir.as_os_str()),
                data_dir,
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

/// The data dir outside the root, if any, and the config files Git reads.
type Admitted = (Option<PathBuf>, Vec<PathBuf>);

fn own_directory(
    entry: &Path,
    reported: &Reported,
    watched: &mut Vec<(PathBuf, Fingerprint)>,
) -> Option<Admitted> {
    (reported.git_dir == entry && reported.common_dir == entry).then_some(())?;
    no_pointers(entry, watched)?;
    Some((None, configs(entry, entry)))
}

fn gitfile(
    root: &Path,
    bytes: &[u8],
    reported: &Reported,
    watched: &mut Vec<(PathBuf, Fingerprint)>,
) -> Option<Admitted> {
    let git_dir = paths::resolve(root, paths::gitfile_path(bytes)?)?;
    real_directories(&git_dir, watched)?;
    (reported.git_dir == git_dir).then_some(())?;
    let ((path, fingerprint), commondir) = watch_reading(git_dir.join("commondir"));
    watched.push((path, fingerprint.clone()));
    match (fingerprint, commondir) {
        // A separate git dir.
        (Fingerprint::Missing, _) => {
            (reported.common_dir == git_dir).then_some(())?;
            no_pointers(&git_dir, watched)?;
            let configs = configs(&git_dir, &git_dir);
            Some((Some(git_dir), configs))
        }
        // A linked worktree of `<common>`, at `<common>/worktrees/<name>`.
        (Fingerprint::Contents { .. }, Some(commondir)) => {
            let common = paths::resolve(&git_dir, paths::commondir_path(&commondir)?)?;
            real_directories(&common, watched)?;
            let in_worktrees = git_dir.parent() == Some(common.join("worktrees").as_path());
            (reported.common_dir == common && in_worktrees).then_some(())?;
            no_pointers(&common, watched)?;
            // The worktree's back-pointer, which `git worktree` maintains.
            let back_pointer = watch(git_dir.join("gitdir"));
            matches!(back_pointer.1, Fingerprint::Contents { .. }).then_some(())?;
            watched.push(back_pointer);
            let configs = configs(&common, &git_dir);
            Some((Some(git_dir), configs))
        }
        _ => None,
    }
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

/// Watches the pointer files a plain git dir must not hold.
fn no_pointers(git_dir: &Path, watched: &mut Vec<(PathBuf, Fingerprint)>) -> Option<()> {
    for name in POINTER_FILES {
        let pointer = watch(git_dir.join(name));
        let missing = pointer.1 == Fingerprint::Missing;
        watched.push(pointer);
        missing.then_some(())?;
    }
    Some(())
}

/// Watches every directory on `path`, which must each be real.
fn real_directories(path: &Path, watched: &mut Vec<(PathBuf, Fingerprint)>) -> Option<()> {
    let mut walked = PathBuf::new();
    for component in path.components() {
        walked.push(component);
        if matches!(component, Component::Normal(_)) {
            let directory = watch(walked.clone());
            let real = directory.1 == Fingerprint::Directory;
            watched.push(directory);
            real.then_some(())?;
        }
    }
    Some(())
}
