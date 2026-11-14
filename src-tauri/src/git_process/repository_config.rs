//! Sage clears the global and system Git config, but Git still honours the
//! repository's own `config` and `config.worktree`. Several keys there name a
//! program Git runs: content filters, diff and merge drivers, transport
//! commands, credential helpers. A folder that ships its own `.git` directory
//! would otherwise run code as soon as Sage refreshes status, so Sage refuses
//! to run Git for a root whose repository config sets any of them. Sage's
//! Git never runs in a submodule repository (see `submodule_isolation`),
//! so submodule configs need no admission.
//!
//! The verdict is cached per root against a length and SHA-256 digest of
//! every file that decides which config Git reads. Each call re-reads those
//! few small files in-process and compares them, so steady-state polling
//! starts no process, and Git itself stays the only config parser.
//!
//! # Threat model
//!
//! This defends against untrusted repository content: a cloned, downloaded,
//! extracted or synced folder whose `.git` config tries to run commands when
//! Sage runs Git there. The admitted layouts are the root's own `.git`
//! directory, a separate git dir and a linked worktree (see [`layout`]).
//! Every `gitdir:` and `commondir` file is read the way Git reads it, and
//! every directory on the paths they name is fingerprinted by the identity of
//! what it reaches, links included, so retargeting a link invalidates the
//! cached verdict. Every cache hit re-checks all of them.
//!
//! A hostile process writing `.git/config` concurrently, between this check
//! and the spawn, is out of scope: it already runs with the user's full
//! privileges, and closing that window would need Git to read its config from
//! a validated snapshot. The next call sees the change, because watched files
//! are compared by their contents.
use std::path::{Path, PathBuf};

use sage_core::error::{AppError, AppResult, RepositoryLayout};

use crate::git_budget;

use super::{GitOutput, GitProcessPolicy, GitRootAuthority, parallel_limits};

mod cache;
mod discovery;
mod fingerprint;
mod layout;
mod paths;
mod policy;

#[cfg(test)]
use cache::{MAX_CACHED_ROOTS, slot_in};
pub(super) use layout::AdmittedRepository;

use discovery::{Location, discovery_path};
use fingerprint::{Fingerprint, watch};
pub(super) use paths::os_path;
use policy::{path_protection_keys, path_protection_refusal, refusal_message};

const GIT_DIR_NAME: &str = ".git";
const POINTER_FILES: [&str; 2] = ["commondir", "gitdir"];
const MAX_LOCATION_BYTES: usize = 64 * 1024;
const MAX_CONFIG_LIST_BYTES: usize = 1024 * 1024;

/// `None` when `root` is in no repository.
pub(super) async fn admit(
    root: &Path,
    authority: Option<&GitRootAuthority>,
) -> AppResult<Option<AdmittedRepository>> {
    let slot = cache::admission_slot(root);
    // Holding the per-root lock makes concurrent first reads share one probe.
    let mut cached = tokio::select! {
        cached = slot.lock() => cached,
        () = super::cancellation::cancelled() => return Err(super::cancellation::error()),
    };
    if let Some(admission) = cached.as_ref().filter(|admission| admission.is_current()) {
        return admission.verdict();
    }
    let Probe {
        admission,
        cacheable,
    } = Probe::run(root, authority).await?;
    let verdict = admission.verdict();
    cache::store(&mut cached, cacheable.then_some(admission));
    verdict
}

/// Admits `root` without checking its config, for tests that use a repository
/// filter as a deterministic way to keep a Git process running.
#[cfg(test)]
pub(crate) fn trust_for_test(root: &Path) {
    let root = root.canonicalize().expect("trusted test root exists");
    cache::trust(
        root,
        Admission {
            watched: Vec::new(),
            refusal: None,
            repository: None,
            accounted: 0,
        },
    );
}

struct Admission {
    watched: Vec<(PathBuf, Fingerprint)>,
    refusal: Option<Refusal>,
    repository: Option<AdmittedRepository>,
    /// The bytes this verdict was counted at in the cache's budget.
    accounted: usize,
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
    fn retained_bytes(&self) -> usize {
        self.watched
            .iter()
            .map(|(path, _)| path.as_os_str().len() + size_of::<(PathBuf, Fingerprint)>())
            .sum()
    }

    fn is_current(&self) -> bool {
        self.watched
            .iter()
            .all(|(path, fingerprint)| Fingerprint::of(path) == *fingerprint)
    }

    fn verdict(&self) -> AppResult<Option<AdmittedRepository>> {
        match &self.refusal {
            Some(refusal) => Err(refusal.clone().into()),
            None => Ok(self.repository.clone()),
        }
    }
}

struct Probe {
    admission: Admission,
    /// False when the fingerprints cannot prove the verdict stays valid.
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
                    accounted: 0,
                },
                cacheable,
            });
        }
        let location = Location::parse(&location)?;
        let (mut watched, cacheable) = discovery_path(ancestors, &location.git_dir);
        let layout = match layout::resolve(root, &location)? {
            Ok(layout) => layout,
            // A layout refusal can rest on files these fingerprints do not
            // cover (`core.worktree`, a stray pointer file), so it is never
            // cached: the next call probes again and sees the repair.
            Err(layout) => {
                return Ok(Self {
                    admission: Admission {
                        watched,
                        refusal: Some(Refusal::Layout(layout)),
                        repository: None,
                        accounted: 0,
                    },
                    cacheable: false,
                });
            }
        };
        let mut repository = layout.repository;
        watched.extend(layout.watched);
        watched.extend(layout.config_files.into_iter().map(watch));
        let refusal = if watched
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
                let refusal = listed_config_refusal(
                    root,
                    authority,
                    &repository,
                    &listing.stdout,
                    refusal_message,
                )
                .await?;
                if refusal.is_none() {
                    repository.parallel = Some(if parallel_limits::lists_any(&listing.stdout) {
                        read_parallel_caps(root, authority, &repository).await
                    } else {
                        parallel_limits::ParallelLimits::default()
                    });
                }
                refusal
            }
        };
        Ok(Self {
            admission: Admission {
                watched,
                refusal,
                repository: Some(repository),
                accounted: 0,
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
                accounted: 0,
            },
            cacheable: false,
        }
    }
}

fn has_unreadable_config(ancestors: &[(PathBuf, Fingerprint)]) -> bool {
    ancestors
        .iter()
        .any(|(entry, fingerprint)| match fingerprint {
            Fingerprint::Unreadable(_) => true,
            Fingerprint::Directory(_) => matches!(
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

async fn read_parallel_caps(
    root: &Path,
    authority: Option<&GitRootAuthority>,
    repository: &AdmittedRepository,
) -> parallel_limits::ParallelLimits {
    let values = probe_git(
        root,
        authority,
        Some(repository),
        &parallel_limits::PROBE_ARGS,
        parallel_limits::MAX_PROBE_BYTES,
    )
    .await;
    parallel_limits::ParallelLimits::from_probe(values.as_ref().ok())
}

/// Turning `core.protectNTFS` or `core.protectHFS` off lets the index hold
/// names the filesystem resolves to another entry, so only an explicit `true`
/// passes.
async fn listed_config_refusal(
    root: &Path,
    authority: Option<&GitRootAuthority>,
    repository: &AdmittedRepository,
    listing: &[u8],
    message: fn(&[u8]) -> Option<String>,
) -> AppResult<Option<Refusal>> {
    if let Some(message) = message(listing) {
        return Ok(Some(Refusal::Config(message)));
    }
    for key in path_protection_keys(listing) {
        let value = probe_git(
            root,
            authority,
            Some(repository),
            &["config", "--type=bool", "--get", key],
            64,
        )
        .await?;
        if !value.status.success() || value.stdout_truncated || value.stdout.trim_ascii() != b"true"
        {
            return Ok(Some(Refusal::Config(path_protection_refusal(key))));
        }
    }
    Ok(None)
}

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

#[cfg(test)]
mod tests;
