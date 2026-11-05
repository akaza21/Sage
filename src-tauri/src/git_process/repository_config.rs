//! Admission check for repository-local Git configuration.
//!
//! Sage clears the global and system Git config, but Git still honours the
//! repository's own `config` and `config.worktree`. Several keys there name a
//! program Git runs: content filters, diff and merge drivers, transport
//! commands, credential helpers. A folder that ships its own `.git` directory
//! would otherwise run code as soon as Sage refreshes status, so Sage refuses
//! to run Git for a root whose repository config sets any of them. The
//! submodules Git would open from that repository get the same check (see
//! [`submodules`]).
//!
//! The verdict is cached per root against the exact bytes of every file that
//! decides which config Git reads. Each call re-reads those few small files
//! in-process and compares them, so steady-state polling starts no process,
//! and Git itself stays the only config parser.
//!
//! # Threat model
//!
//! This defends against untrusted repository content: a cloned, downloaded,
//! extracted or synced folder whose `.git` config tries to run commands when
//! Sage runs Git there. Only the root's own `.git` directory is admitted, as
//! both git dir and common dir, and it may not be a symlink or junction, so
//! every location Git reads is a fixed name below the identity-checked root
//! handle. A `.git` file, or a `commondir` or `gitdir` file inside `.git`, is
//! refused by its presence, wherever it resolves: it could route through an
//! intermediate directory or link that is later repointed outside the root
//! without changing any watched bytes. Every cache hit re-checks that `.git`
//! is still a plain directory (on Windows, not any reparse point) and that
//! neither pointer file has appeared.
//!
//! A hostile process writing `.git/config` concurrently, between this check
//! and the spawn, is out of scope: it already runs with the user's full
//! privileges, and closing that window would need Git to read its config from
//! a validated snapshot. The next call sees the change, because watched files
//! are compared by their bytes. Admitting `.git` files, linked worktrees and
//! Git data outside the root is tracked in issue #87.

use std::{
    collections::HashMap,
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
};

use crate::{
    error::{AppError, AppResult, RepositoryLayout},
    git_budget,
};

use super::{GitOutput, GitProcessPolicy, GitRootAuthority};

mod fingerprint;
mod policy;
mod submodules;

use fingerprint::{Fingerprint, watch};
use policy::refusal_message;
use submodules::Scan;

const GIT_DIR_NAME: &str = ".git";
/// Files inside a git dir that make Git read repository data from elsewhere.
const POINTER_FILES: [&str; 2] = ["commondir", "gitdir"];
const MAX_LOCATION_BYTES: usize = 64 * 1024;
const MAX_CONFIG_LIST_BYTES: usize = 1024 * 1024;
/// Idle entries are dropped once this many roots are cached.
const MAX_CACHED_ROOTS: usize = 256;

type AdmissionSlot = Arc<tokio::sync::Mutex<Option<Admission>>>;

static ADMISSIONS: LazyLock<Mutex<HashMap<PathBuf, AdmissionSlot>>> = LazyLock::new(Mutex::default);

/// The repository admission inspected. Admitted Git calls run with these
/// explicit locations, so they open exactly this repository.
///
/// Only a repository whose work tree is the opened root and whose git data is
/// the root's own `.git` directory is admitted, and both locations are
/// relative to it. The child's working
/// directory is the identity-checked root handle, so a directory swapped in
/// at the root's name, or a move of the root, cannot redirect them. Git data
/// reached any other way (a `gitdir:` file, a `commondir` pointer, a linked
/// worktree, or a root nested below the top level) is refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AdmittedRepository {
    git_dir: PathBuf,
    work_tree: PathBuf,
}

impl AdmittedRepository {
    fn inside(root: &Path, location: &Location) -> AppResult<Result<Self, RepositoryLayout>> {
        let root = root.canonicalize()?;
        if location.work_tree.canonicalize()? != root {
            return Ok(Err(RepositoryLayout::NestedRoot));
        }
        // Only the root's own `.git` directory is admitted, as both the git
        // dir and the common dir. A `gitdir:` file or a `commondir` pointer
        // can name a path through an intermediate directory that is later
        // swapped for a link outside the root without changing any watched
        // bytes.
        let git_dir = PathBuf::from(GIT_DIR_NAME);
        let own = root.join(&git_dir);
        if location.git_dir.canonicalize()? != own || location.common_dir.canonicalize()? != own {
            return Ok(Err(RepositoryLayout::ExternalGitDir));
        }
        Ok(Ok(Self {
            git_dir,
            work_tree: PathBuf::from(Component::CurDir.as_os_str()),
        }))
    }

    pub(super) fn git_dir(&self) -> &Path {
        &self.git_dir
    }

    pub(super) fn work_tree(&self) -> &Path {
        &self.work_tree
    }
}

/// Fails with [`AppError::UnsafeRepositoryConfig`] when the repository that
/// Git would open at `root` configures a command-running key. Otherwise
/// returns that repository, or `None` when `root` is in no repository.
pub(super) async fn admit(
    root: &Path,
    authority: Option<&GitRootAuthority>,
) -> AppResult<Option<AdmittedRepository>> {
    let slot = admission_slot(root);
    // Holding the per-root lock makes concurrent first reads share one probe.
    let mut cached = slot.lock().await;
    if let Some(admission) = cached.as_mut().filter(|admission| admission.is_current()) {
        admission.check_submodules(root, authority).await?;
        return admission.verdict();
    }
    let Probe {
        mut admission,
        cacheable,
    } = Probe::run(root, authority).await?;
    admission.check_submodules(root, authority).await?;
    let verdict = admission.verdict();
    *cached = cacheable.then_some(admission);
    verdict
}

fn admission_slot(root: &Path) -> AdmissionSlot {
    slot_in(
        &mut ADMISSIONS.lock().unwrap_or_else(PoisonError::into_inner),
        root,
    )
}

/// Returns the slot for `root`, first dropping every idle slot when the map is
/// full. Slots in use stay, so an in-flight probe keeps its lock. When every
/// slot is in use, `root` gets a private slot, so its admission is not cached.
fn slot_in(admissions: &mut HashMap<PathBuf, AdmissionSlot>, root: &Path) -> AdmissionSlot {
    if !admissions.contains_key(root) && admissions.len() >= MAX_CACHED_ROOTS {
        admissions.retain(|_, slot| Arc::strong_count(slot) > 1);
        if admissions.len() >= MAX_CACHED_ROOTS {
            return AdmissionSlot::default();
        }
    }
    Arc::clone(admissions.entry(root.to_path_buf()).or_default())
}

/// Admits `root` without checking its config, for tests that use a repository
/// filter as a deterministic way to keep a Git process running.
#[cfg(test)]
pub(crate) fn trust_for_test(root: &Path) {
    let root = root.canonicalize().expect("trusted test root exists");
    let slot = AdmissionSlot::new(tokio::sync::Mutex::new(Some(Admission {
        watched: Vec::new(),
        refusal: None,
        repository: None,
        submodules: None,
    })));
    ADMISSIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(root, slot);
}

struct Admission {
    watched: Vec<(PathBuf, Fingerprint)>,
    refusal: Option<Refusal>,
    repository: Option<AdmittedRepository>,
    /// The submodules Git would open from `repository`, checked lazily.
    submodules: Option<Scan>,
}

#[derive(Clone)]
enum Refusal {
    /// The public message naming the keys.
    Config(String),
    Layout(RepositoryLayout),
}

impl From<Refusal> for AppError {
    fn from(refusal: Refusal) -> Self {
        match refusal {
            Refusal::Config(message) => Self::UnsafeRepositoryConfig(message),
            Refusal::Layout(layout) => Self::RepositoryOutsideRoot(layout),
        }
    }
}

impl Admission {
    fn is_current(&self) -> bool {
        self.watched
            .iter()
            .all(|(path, fingerprint)| Fingerprint::of(path) == *fingerprint)
    }

    fn verdict(&self) -> AppResult<Option<AdmittedRepository>> {
        let submodules = self.submodules.as_ref().and_then(Scan::refusal);
        match self.refusal.as_ref().or(submodules) {
            Some(refusal) => Err(refusal.clone().into()),
            None => Ok(self.repository.clone()),
        }
    }

    /// Scans the submodules of an admitted repository again once anything the
    /// last scan relied on has changed.
    async fn check_submodules(
        &mut self,
        root: &Path,
        authority: Option<&GitRootAuthority>,
    ) -> AppResult<()> {
        let Some(repository) = self.repository.as_ref().filter(|_| self.refusal.is_none()) else {
            return Ok(());
        };
        if !self.submodules.as_ref().is_some_and(Scan::is_current) {
            self.submodules = Some(Scan::run(root, authority, repository).await?);
        }
        Ok(())
    }
}

struct Probe {
    admission: Admission,
    /// False when the fingerprints cannot prove the verdict stays valid, so
    /// the next call probes again.
    cacheable: bool,
}

impl Probe {
    async fn run(root: &Path, authority: Option<&GitRootAuthority>) -> AppResult<Self> {
        #[cfg(test)]
        tests::record_probe(root);
        // Everything is fingerprinted before Git reads it, so a change made
        // mid-probe invalidates the result on the next call.
        let ancestors = root
            .ancestors()
            .map(|directory| watch(directory.join(GIT_DIR_NAME)))
            .collect::<Vec<_>>();
        let unreadable_before_probe = has_unreadable_config(&ancestors);
        let location = probe_git(
            root,
            authority,
            None,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
                "--show-toplevel",
            ],
            MAX_LOCATION_BYTES,
        )
        .await?;
        if !location.status.success() {
            // Git may have failed only because it could not read a config.
            // Running Git unpinned after that would read a config nobody
            // checked, so this fails closed and probes again next time.
            if unreadable_before_probe || has_unreadable_config(&ancestors) {
                return Ok(Self::uncached_refusal(ancestors, UNREADABLE));
            }
            // Git found no repository, so it reads no repository config. That
            // stays true only while no `.git` exists on the discovery path: an
            // existing but invalid one (an empty directory, say) can become a
            // repository without any watched file changing.
            let cacheable = ancestors
                .iter()
                .all(|(_, fingerprint)| *fingerprint == Fingerprint::Missing);
            return Ok(Self {
                admission: Admission {
                    watched: ancestors,
                    refusal: None,
                    repository: None,
                    submodules: None,
                },
                cacheable,
            });
        }
        let location = Location::parse(&location)?;
        let (mut watched, cacheable) = discovery_path(ancestors, &location.git_dir);
        let repository = match AdmittedRepository::inside(root, &location)? {
            Ok(repository) => repository,
            Err(layout) => {
                return Ok(Self {
                    admission: Admission {
                        watched,
                        refusal: Some(Refusal::Layout(layout)),
                        repository: None,
                        submodules: None,
                    },
                    cacheable,
                });
            }
        };
        let Location { git_dir, .. } = location;
        // Git follows these files wherever they lead, and a link they pass
        // through can be repointed without changing their bytes, so their
        // mere presence is refused. Watching them keeps a cached admission
        // from outliving one created later.
        let pointers = POINTER_FILES
            .iter()
            .map(|name| watch(git_dir.join(name)))
            .collect::<Vec<_>>();
        let has_pointer = pointers
            .iter()
            .any(|(_, fingerprint)| *fingerprint != Fingerprint::Missing);
        watched.extend(pointers);
        watched.extend(
            [git_dir.join("config"), git_dir.join("config.worktree")]
                .into_iter()
                .map(watch),
        );
        let refusal = if has_pointer
            || watched
                .iter()
                .any(|(_, fingerprint)| *fingerprint == Fingerprint::Link)
        {
            Some(Refusal::Layout(RepositoryLayout::ExternalGitDir))
        } else if watched
            .iter()
            .any(|(_, fingerprint)| matches!(fingerprint, Fingerprint::Unreadable(_)))
        {
            return Ok(Self::uncached_refusal(watched, UNREADABLE));
        } else if watched
            .iter()
            .any(|(_, fingerprint)| fingerprint.is_unverifiable())
        {
            Some(Refusal::Config(TOO_LARGE.to_owned()))
        } else {
            let listing = probe_git(
                root,
                authority,
                Some(&repository),
                &["config", "--list", "--no-includes", "--name-only", "-z"],
                MAX_CONFIG_LIST_BYTES,
            )
            .await?;
            if !listing.status.success() {
                // Git's stderr names local paths, so it never reaches the
                // public message; the next call probes again.
                return Ok(Self::uncached_refusal(watched, UNREADABLE));
            }
            if listing.stdout_truncated {
                Some(Refusal::Config(TOO_LARGE.to_owned()))
            } else {
                refusal_message(&listing.stdout).map(Refusal::Config)
            }
        };
        Ok(Self {
            admission: Admission {
                watched,
                refusal,
                repository: Some(repository),
                submodules: None,
            },
            cacheable,
        })
    }

    fn uncached_refusal(watched: Vec<(PathBuf, Fingerprint)>, message: &str) -> Self {
        Self {
            admission: Admission {
                watched,
                refusal: Some(Refusal::Config(message.to_owned())),
                repository: None,
                submodules: None,
            },
            cacheable: false,
        }
    }
}

/// True when a `.git` entry on the discovery path, or the `config` inside a
/// `.git` directory there, exists but cannot be read.
fn has_unreadable_config(ancestors: &[(PathBuf, Fingerprint)]) -> bool {
    ancestors
        .iter()
        .any(|(entry, fingerprint)| match fingerprint {
            Fingerprint::Unreadable(_) => true,
            Fingerprint::Directory => matches!(
                Fingerprint::of(&entry.join("config")),
                Fingerprint::Unreadable(_)
            ),
            _ => false,
        })
}

const UNREADABLE: &str = "Sage did not run Git here because it could not read this repository's \
     Git config to check it. Use the terminal for Git in this folder.";

const TOO_LARGE: &str = "Sage did not run Git here because this repository's Git config is too \
     large or unusual to check. Use the terminal for Git in this folder.";

async fn probe_git(
    root: &Path,
    authority: Option<&GitRootAuthority>,
    repository: Option<&AdmittedRepository>,
    args: &[&str],
    stdout_limit: usize,
) -> AppResult<GitOutput> {
    let authority = authority.map(GitRootAuthority::try_clone).transpose()?;
    super::run_admitted(
        root,
        args,
        stdout_limit,
        GitProcessPolicy::read(git_budget::READ),
        authority,
        repository,
    )
    .await
}

/// Keeps the `.git` entries from the root up to the one Git used. The verdict
/// is cacheable only when every entry below that one is missing, because an
/// existing entry Git skipped could later become the repository it opens.
fn discovery_path(
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
        Fingerprint::Directory => entry.to_path_buf(),
        Fingerprint::Contents(bytes) => match gitfile_target(entry, bytes) {
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

/// The directory a `gitdir: <path>` file names, relative to the file.
fn gitfile_target(entry: &Path, bytes: &[u8]) -> Option<PathBuf> {
    let text = std::str::from_utf8(bytes).ok()?;
    let target = text.strip_prefix("gitdir:")?.trim();
    let parent = entry.parent()?;
    (!target.is_empty()).then(|| parent.join(target))
}

struct Location {
    git_dir: PathBuf,
    common_dir: PathBuf,
    work_tree: PathBuf,
}

impl Location {
    fn parse(output: &GitOutput) -> AppResult<Self> {
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

#[cfg(test)]
mod tests;
