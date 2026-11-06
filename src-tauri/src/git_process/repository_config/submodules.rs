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

use super::super::readers::RecordFilter;
use super::{
    AdmittedRepository, Fingerprint, GIT_DIR_NAME, GitRootAuthority, MAX_CONFIG_LIST_BYTES,
    POINTER_FILES, Refusal, StdoutCapture, listed_config_refusal,
    policy::submodule_refusal_message, probe_git,
};

/// Gitlinks checked across the whole tree of submodules.
const MAX_SUBMODULES: usize = 64;
/// Submodules nested deeper than this are refused.
const MAX_SUBMODULE_DEPTH: usize = 4;
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
            let index = canonical.join(&repository.git_dir).join("index");
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
                pending.push_back((submodule, depth + 1));
            }
        }
        Ok(None)
    }

    /// Resolves the repository Git would open for the gitlink at `work_tree`
    /// (root-relative). Every path component Git passes through is
    /// fingerprinted and must be a real directory, never a symlink or
    /// junction, so the operating system resolves exactly the path inspected
    /// here and a later retarget invalidates the cached verdict.
    fn open(&mut self, root: &Path, work_tree: PathBuf) -> Opened {
        let mut walked = PathBuf::new();
        for component in work_tree.components() {
            walked.push(component);
            match self.watch(root.join(&walked)) {
                Fingerprint::Directory => {}
                // Git looks for `.git` below the gitlink path, which a file
                // (a type change in progress, say) cannot hold.
                Fingerprint::Missing | Fingerprint::Contents(_) | Fingerprint::Oversized => {
                    return Opened::Unpopulated;
                }
                _ => return Opened::Refused(uncheckable()),
            }
        }
        let git_dir = match self.watch(root.join(&work_tree).join(GIT_DIR_NAME)) {
            Fingerprint::Missing => return Opened::Unpopulated,
            Fingerprint::Directory => work_tree.join(GIT_DIR_NAME),
            Fingerprint::Contents(bytes) => {
                let Some(target) = gitfile_target(&work_tree, &bytes) else {
                    return Opened::Refused(uncheckable());
                };
                // `.git` itself is the root's own git dir, checked by the
                // repository's admission; everything below it is checked here.
                let mut walked = PathBuf::from(GIT_DIR_NAME);
                for component in target.components().skip(1) {
                    walked.push(component);
                    if self.watch(root.join(&walked)) != Fingerprint::Directory {
                        return Opened::Refused(uncheckable());
                    }
                }
                target
            }
            _ => return Opened::Refused(uncheckable()),
        };
        let absolute_git_dir = root.join(&git_dir);
        for name in POINTER_FILES {
            if self.watch(absolute_git_dir.join(name)) != Fingerprint::Missing {
                return Opened::Refused(uncheckable());
            }
        }
        for name in ["config", "config.worktree"] {
            if !matches!(
                self.watch(absolute_git_dir.join(name)),
                Fingerprint::Missing | Fingerprint::Contents(_)
            ) {
                return Opened::Refused(uncheckable());
            }
        }
        // A submodule's own parallel-job keys are not capped here: the caps
        // the parent passes with `-c` reach its Git children too.
        Opened::Repository(AdmittedRepository {
            git_dir,
            work_tree,
            parallel: Vec::new(),
        })
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
/// stays inside the work tree; see [`ambiguous_component`].
fn join_relative(base: &Path, path: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(path).ok()?;
    let mut joined = base
        .components()
        .filter(|component| !matches!(component, Component::CurDir))
        .collect::<PathBuf>();
    for part in text.split('/') {
        if ambiguous_component(part) || part.eq_ignore_ascii_case(GIT_DIR_NAME) {
            return None;
        }
        joined.push(part);
    }
    Some(joined)
}

/// The root-relative git dir a `gitdir:` file in the root-relative
/// `work_tree` names, in the only shape Git writes for an absorbed
/// submodule: `..` components that climb exactly to the root, then
/// `.git/modules/<name>...`. The climb passes only directories
/// [`Scan::open`] has proved are real, so resolving it lexically matches the
/// operating system; any other `..` is refused, since after a symlink it
/// would resolve somewhere else entirely.
///
/// The file is read as Git's `read_gitfile` reads it: one trailing `\n`, then
/// one trailing `\r`, are dropped, the rest must start with `gitdir: `, and
/// everything after that is the path byte for byte, split on `/` (and on
/// Windows also `\`). Each named component must pass
/// [`ambiguous_component`].
fn gitfile_target(work_tree: &Path, bytes: &[u8]) -> Option<PathBuf> {
    let line = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let line = line.strip_suffix(b"\r").unwrap_or(line);
    let target = std::str::from_utf8(line.strip_prefix(b"gitdir: ")?).ok()?;
    if Path::new(target).has_root() {
        return None;
    }
    #[cfg(windows)]
    let parts = target.split(['/', '\\']).collect::<Vec<_>>();
    #[cfg(not(windows))]
    let parts = target.split('/').collect::<Vec<_>>();
    let climbs = parts.iter().take_while(|part| **part == "..").count();
    let rest = &parts[climbs..];
    let well_formed = climbs == work_tree.components().count()
        && rest.len() >= 3
        && rest[0] == GIT_DIR_NAME
        && rest[1] == "modules"
        && !rest.iter().any(|part| ambiguous_component(part));
    well_formed.then(|| rest.iter().collect())
}

/// True when Git and the filesystem calls here could resolve `part` to
/// different entries, or when it is not one plain name at all. Fingerprints
/// use verbatim (`\\?\`) paths on Windows, which skip every Win32
/// normalisation, so each name Win32 would rewrite is refused instead:
/// a trailing `.` or space, a reserved device name, an 8.3 short-name alias
/// (`~` and a digit) and an alternate data stream (`:`). Everywhere, a
/// backslash, edge whitespace, control characters (NUL included) and the
/// code points HFS+ ignores are refused, and so are empty, `.` and `..`.
fn ambiguous_component(part: &str) -> bool {
    let ignorable = |c: char| matches!(c, '\u{200c}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{206a}'..='\u{206f}' | '\u{feff}');
    let short_name = part
        .as_bytes()
        .windows(2)
        .any(|pair| pair[0] == b'~' && pair[1].is_ascii_digit());
    let win32_rewrites = part.ends_with('.') || short_name || reserved_device_name(part);
    matches!(part, "" | "." | "..")
        || part.trim() != part
        || (cfg!(windows) && win32_rewrites)
        || part
            .chars()
            .any(|c| c.is_control() || matches!(c, '\\' | ':') || ignorable(c))
}

/// `CON`, `PRN`, `AUX`, `NUL`, `COM0`-`COM9`, `LPT0`-`LPT9` (and their
/// superscript-digit forms), `CONIN$` and `CONOUT$`, with or without an
/// extension, in any case.
fn reserved_device_name(part: &str) -> bool {
    let stem = part.split('.').next().unwrap_or(part).to_ascii_uppercase();
    match stem.as_str() {
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$" => true,
        _ => ["COM", "LPT"].iter().any(|prefix| {
            stem.strip_prefix(prefix).is_some_and(|digit| {
                (digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit())
                    || matches!(digit, "\u{b9}" | "\u{b2}" | "\u{b3}")
            })
        }),
    }
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

    fn target(bytes: &[u8]) -> Option<PathBuf> {
        gitfile_target(Path::new("sub"), bytes)
    }

    #[test]
    fn only_git_line_endings_are_stripped() {
        let expected = Some(PathBuf::from(".git/modules/sub"));
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
            assert_eq!(separated, Some(PathBuf::from(".git/modules/sub")));
            assert_eq!(named, Some(PathBuf::from(".git/modules/foo/bar")));
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
