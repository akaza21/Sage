//! Admission for the submodule repositories Git opens on its own.
//!
//! `git status` and `git diff` check every populated submodule by running Git
//! inside it, and that child reads the submodule's own config. A downloaded
//! folder could ship a submodule whose config names a filter or other program,
//! so before Sage runs Git in an admitted repository it checks every submodule
//! Git would open, recursively, with the same key policy as the repository.
//!
//! A submodule is the gitlink entries in an index whose work tree holds a
//! `.git`: either a directory there, or a `gitdir:` file naming a directory
//! below the root's own `.git/modules`. Anything else Git could open (Git data
//! elsewhere, pointer files, links, special files) is refused, as are more
//! than [`MAX_SUBMODULES`] gitlinks, nesting deeper than
//! [`MAX_SUBMODULE_DEPTH`], or an index of more than [`MAX_INDEX_RECORDS`]
//! entries. The index listing is streamed and only gitlink records are kept,
//! so memory does not grow with the index. Refusals here never name a path.
//!
//! The verdict is cached against the same byte fingerprints as the
//! repository's own admission, plus a metadata stamp of each index read: an
//! index is too large to compare by bytes, and every Git write replaces it.

use std::{
    collections::{BTreeSet, VecDeque},
    fs,
    path::{Component, Path, PathBuf},
    time::SystemTime,
};

use crate::error::AppResult;

use super::super::process_tree::{MAX_TREE_PROCESSES, PROCESSES_PER_JOB, TREE_HEADROOM};
use super::super::readers::RecordFilter;
use super::{
    AdmittedRepository, Fingerprint, GIT_DIR_NAME, GitRootAuthority, MAX_CONFIG_LIST_BYTES,
    POINTER_FILES, Refusal, StdoutCapture, fingerprint::watch_reading, layout::walk,
    listed_config_refusal, paths, policy::submodule_refusal_message, probe_git,
};

/// Gitlinks checked across the whole tree of submodules.
const MAX_SUBMODULES: usize = 64;
/// Submodules nested deeper than this are refused.
const MAX_SUBMODULE_DEPTH: usize = 4;

// A recursive fetch runs one job per level once submodules exist (see
// `parallel_limits`), so its whole chain must fit the Git job's process limit.
const _: () = assert!(
    (MAX_SUBMODULE_DEPTH as u32 + 1) * PROCESSES_PER_JOB + TREE_HEADROOM <= MAX_TREE_PROCESSES
);
/// Index entries listed per repository: the workspace walk limit.
const MAX_INDEX_RECORDS: usize = 200_000;
/// One `ls-files --stage` record: mode, object name, stage and a path.
const MAX_INDEX_RECORD_BYTES: usize = 128 * 1024;
const GITLINK: &[u8] = b"160000 ";

pub(super) const UNCHECKABLE: &str = "Sage did not run Git here because this repository's submodules are \
     too many, too deeply nested, or too unusual to check. Use the terminal for Git in this \
     folder.";

pub(super) struct Scan {
    watched: Vec<(PathBuf, Fingerprint)>,
    indexes: Vec<(PathBuf, Option<IndexStamp>)>,
    refusal: Option<Refusal>,
    /// False when Git failed mid-scan, so the next call scans again.
    cacheable: bool,
    /// Whether Git would open at least one submodule.
    populated: bool,
}

enum Opened {
    Unpopulated,
    Repository(AdmittedRepository),
    Refused(Refusal),
}

impl Scan {
    pub(super) async fn run(
        root: &Path,
        authority: Option<&GitRootAuthority>,
        repository: &AdmittedRepository,
    ) -> AppResult<Self> {
        let mut scan = Self {
            watched: Vec::new(),
            indexes: Vec::new(),
            refusal: None,
            cacheable: true,
            populated: false,
        };
        scan.refusal = scan.walk(root, authority, repository).await?;
        Ok(scan)
    }

    pub(super) fn is_current(&self) -> bool {
        self.cacheable
            && self
                .watched
                .iter()
                .all(|(path, fingerprint)| Fingerprint::of(path) == *fingerprint)
            && self
                .indexes
                .iter()
                .all(|(path, stamp)| IndexStamp::of(path) == *stamp)
    }

    /// Roughly what this scan keeps alive: its watched paths and stamps.
    pub(super) fn retained_bytes(&self) -> usize {
        let watched = self
            .watched
            .iter()
            .map(|(path, _)| path.as_os_str().len() + size_of::<(PathBuf, Fingerprint)>());
        let indexes = self
            .indexes
            .iter()
            .map(|(path, _)| path.as_os_str().len() + size_of::<(PathBuf, Option<IndexStamp>)>());
        watched.chain(indexes).sum()
    }

    pub(super) fn has_submodules(&self) -> bool {
        self.populated
    }

    pub(super) fn refusal(&self) -> Option<&Refusal> {
        self.refusal.as_ref()
    }

    async fn walk(
        &mut self,
        root: &Path,
        authority: Option<&GitRootAuthority>,
        top: &AdmittedRepository,
    ) -> AppResult<Option<Refusal>> {
        // Fingerprints use canonical paths; Git still runs at the caller's root.
        let canonical = root.canonicalize()?;
        let mut pending = VecDeque::from([(top.clone(), 0_usize)]);
        let mut found = 0_usize;
        while let Some((repository, depth)) = pending.pop_front() {
            let index = repository.data_dir(&canonical).join("index");
            let stamp = IndexStamp::of(&index);
            self.indexes.push((index, stamp));
            // Streamed: only gitlink records are kept, and the listing fails
            // closed at the first gitlink past the remaining budget.
            let listing = probe_git(
                root,
                authority,
                Some(&repository),
                &["ls-files", "--stage", "--full-name", "-z"],
                StdoutCapture::Records(RecordFilter {
                    keep: |record| record.starts_with(GITLINK),
                    max_kept: MAX_SUBMODULES - found,
                    max_records: MAX_INDEX_RECORDS,
                    max_record_bytes: MAX_INDEX_RECORD_BYTES,
                }),
            )
            .await?;
            if !listing.status.success() {
                self.cacheable = false;
                return Ok(Some(uncheckable()));
            }
            if listing.stdout_truncated {
                return Ok(Some(uncheckable()));
            }
            for path in gitlinks(&listing.stdout) {
                found += 1;
                let Some(work_tree) = (found <= MAX_SUBMODULES)
                    .then(|| join_relative(&repository.work_tree, path))
                    .flatten()
                else {
                    return Ok(Some(uncheckable()));
                };
                let submodule = match self.open(&canonical, work_tree) {
                    Opened::Unpopulated => continue,
                    Opened::Refused(refusal) => return Ok(Some(refusal)),
                    Opened::Repository(submodule) => submodule,
                };
                if depth + 1 > MAX_SUBMODULE_DEPTH {
                    return Ok(Some(uncheckable()));
                }
                if let Some(refusal) = self.check_config(root, authority, &submodule).await? {
                    return Ok(Some(refusal));
                }
                self.populated = true;
                pending.push_back((submodule, depth + 1));
            }
        }
        Ok(None)
    }

    /// Resolves the repository Git would open for the gitlink at `work_tree`
    /// (root-relative), the way Git resolves it: through any symlink or
    /// junction, to the `.git` directory there or the git dir its `gitdir:`
    /// file names. That resolved git dir is what is inspected, and every
    /// directory on the way is stamped with the identity of what it reaches,
    /// so a later retarget invalidates the cached verdict.
    fn open(&mut self, root: &Path, work_tree: PathBuf) -> Opened {
        let mut walked = PathBuf::new();
        for component in work_tree.components() {
            walked.push(component);
            match self.watch(root.join(&walked)) {
                Fingerprint::Directory(_) => {}
                // Git looks for `.git` below the gitlink path, which a file
                // (a type change in progress, say) cannot hold.
                Fingerprint::Missing | Fingerprint::Contents { .. } | Fingerprint::Oversized => {
                    return Opened::Unpopulated;
                }
                _ => return Opened::Refused(uncheckable()),
            }
        }
        let entry = root.join(&work_tree).join(GIT_DIR_NAME);
        let (fingerprint, bytes) = self.watch_reading(entry.clone());
        let data_dir = match (fingerprint, bytes) {
            (Fingerprint::Missing, _) => return Opened::Unpopulated,
            (Fingerprint::Directory(_), _) => entry.canonicalize().ok(),
            (Fingerprint::Contents { .. }, Some(bytes)) => {
                paths::gitfile_path(&bytes).and_then(|written| {
                    walk(&root.join(&work_tree), written, &mut |path| {
                        self.watch(path)
                    })
                })
            }
            _ => None,
        };
        let Some(data_dir) = data_dir else {
            return Opened::Refused(uncheckable());
        };
        for name in POINTER_FILES {
            if self.watch(data_dir.join(name)) != Fingerprint::Missing {
                return Opened::Refused(uncheckable());
            }
        }
        for name in ["config", "config.worktree"] {
            if !matches!(
                self.watch(data_dir.join(name)),
                Fingerprint::Missing | Fingerprint::Contents { .. }
            ) {
                return Opened::Refused(uncheckable());
            }
        }
        // Git is pointed at the submodule's `.git` entry, which it follows
        // to the git dir inspected here. Git resolves a relative `gitdir:`
        // against the part of `GIT_DIR` before its last `/`, so the entry is
        // named with `/` separators on every platform.
        let entry = format!(
            "{}/{GIT_DIR_NAME}",
            work_tree.to_string_lossy().replace('\\', "/")
        );
        Opened::Repository(AdmittedRepository::submodule(
            PathBuf::from(entry),
            work_tree,
            data_dir,
        ))
    }
    async fn check_config(
        &mut self,
        root: &Path,
        authority: Option<&GitRootAuthority>,
        submodule: &AdmittedRepository,
    ) -> AppResult<Option<Refusal>> {
        let listing = probe_git(
            root,
            authority,
            Some(submodule),
            &["config", "--list", "--no-includes", "--name-only", "-z"],
            StdoutCapture::Bytes(MAX_CONFIG_LIST_BYTES),
        )
        .await?;
        if !listing.status.success() {
            self.cacheable = false;
            return Ok(Some(uncheckable()));
        }
        if listing.stdout_truncated {
            return Ok(Some(uncheckable()));
        }
        listed_config_refusal(
            root,
            authority,
            submodule,
            &listing.stdout,
            submodule_refusal_message,
        )
        .await
    }

    /// Like [`Self::watch`], also returning a regular file's bytes to parse now.
    fn watch_reading(&mut self, path: PathBuf) -> (Fingerprint, Option<Vec<u8>>) {
        let ((path, fingerprint), bytes) = watch_reading(path);
        self.watched.push((path, fingerprint.clone()));
        (fingerprint, bytes)
    }

    /// Records `path` and returns what it held.
    fn watch(&mut self, path: PathBuf) -> Fingerprint {
        let fingerprint = Fingerprint::of(&path);
        let held = fingerprint.clone();
        self.watched.push((path, fingerprint));
        held
    }
}

fn uncheckable() -> Refusal {
    Refusal::Config(UNCHECKABLE.to_owned())
}

/// The distinct paths of the gitlink records the listing kept.
fn gitlinks(listing: &[u8]) -> BTreeSet<&[u8]> {
    listing
        .split(|byte| *byte == 0)
        .filter(|record| record.starts_with(GITLINK))
        .filter_map(|record| {
            let tab = record.iter().position(|byte| *byte == b'\t')?;
            Some(&record[tab + 1..])
        })
        .collect()
}

/// Joins a `/`-separated index path below `base`. Every component must name
/// one entry the same way for Git and for the checks here, so the result
/// stays inside the work tree; see [`paths::ambiguous_component`].
fn join_relative(base: &Path, path: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(path).ok()?;
    let mut joined = base
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect::<PathBuf>();
    for part in text.split('/') {
        if paths::ambiguous_component(part) || part.eq_ignore_ascii_case(GIT_DIR_NAME) {
            return None;
        }
        joined.push(part);
    }
    Some(joined)
}

/// What an index file's metadata said when it was listed. Git replaces the
/// index on every write, so its size, time and (on Unix) inode all move.
#[derive(PartialEq, Eq)]
struct IndexStamp {
    len: u64,
    modified: Option<SystemTime>,
    #[cfg(unix)]
    inode: (u64, u64, i64, i64),
}

impl IndexStamp {
    fn of(path: &Path) -> Option<Self> {
        let metadata = fs::symlink_metadata(path).ok()?;
        #[cfg(unix)]
        let inode = {
            use std::os::unix::fs::MetadataExt;
            (
                metadata.dev(),
                metadata.ino(),
                metadata.ctime(),
                metadata.ctime_nsec(),
            )
        };
        Some(Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            inode,
        })
    }
}

#[cfg(test)]
mod gitfile_tests {
    use super::*;

    /// The path a submodule gitfile in `sub` names, as walked from there.
    fn target(bytes: &[u8]) -> Option<PathBuf> {
        let written = paths::gitfile_path(bytes)?;
        paths::written_prefixes(Path::new("sub"), written)?.pop()
    }

    #[test]
    fn only_git_line_endings_are_stripped() {
        let expected = Some(PathBuf::from("sub/../.git/modules/sub"));
        for bytes in [
            &b"gitdir: ../.git/modules/sub"[..],
            b"gitdir: ../.git/modules/sub\n",
            b"gitdir: ../.git/modules/sub\r\n",
        ] {
            assert_eq!(target(bytes), expected, "{bytes:?}");
        }
    }

    #[test]
    fn paths_git_could_read_differently_are_refused() {
        for bytes in [
            &b"gitdir: ../.git/modules/sub \n"[..],
            b"gitdir: ../.git/modules/ sub\n",
            b"gitdir: ../.git/modules/sub\t\n",
            b"gitdir: ../.git/modules/sub\0x\n",
            b"gitdir: ../.git/modules/sub\n\n",
            b"gitdir: ../.git/modules/s\xffb\n",
            b"gitdir:../.git/modules/sub\n",
            b"gitdir:  ../.git/modules/sub\n",
        ] {
            assert_eq!(target(bytes), None, "{bytes:?}");
        }
    }

    #[test]
    fn a_backslash_is_a_separator_only_on_windows() {
        let separated = target(b"gitdir: ..\\.git\\modules\\sub\n");
        let named = target(b"gitdir: ../.git/modules/foo\\bar\n");
        if cfg!(windows) {
            assert_eq!(separated, Some(PathBuf::from("sub/../.git/modules/sub")));
            assert_eq!(named, Some(PathBuf::from("sub/../.git/modules/foo/bar")));
        } else {
            assert_eq!(separated, None);
            assert_eq!(named, None);
        }
    }

    #[cfg(windows)]
    #[test]
    fn a_component_win32_would_strip_is_refused() {
        assert_eq!(target(b"gitdir: ../.git/modules/sub.\n"), None);
    }

    fn joined(path: &str) -> Option<PathBuf> {
        join_relative(Path::new(""), path.as_bytes())
    }

    #[test]
    fn gitlink_paths_that_could_name_another_entry_are_refused_everywhere() {
        assert_eq!(joined("lib/sub"), Some(PathBuf::from("lib/sub")));
        for path in [
            "sub ",
            " sub",
            "lib/sub\t",
            "a\\b",
            "sub:stream",
            "sub\0",
            "s\u{200c}ub",
            "lib/.GIT/x",
            "lib//sub",
            "lib/../sub",
        ] {
            assert_eq!(joined(path), None, "{path:?}");
        }
    }

    #[test]
    fn names_win32_would_rewrite_are_refused_only_on_windows() {
        for path in [
            "sub.",
            "lib/aux",
            "nul.txt",
            "COM1",
            "lpt\u{b9}",
            "CONIN$",
            "SUBMOD~1",
        ] {
            let refused = joined(path).is_none();
            if cfg!(windows) {
                assert!(refused, "{path:?}");
            } else {
                assert!(!refused, "{path:?}");
            }
        }
    }
}
