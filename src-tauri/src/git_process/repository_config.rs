//! Admission check for repository-local Git configuration.
//!
//! Sage clears the global and system Git config, but Git still honours the
//! repository's own `config` and `config.worktree`. Several keys there name a
//! program Git runs: content filters, diff and merge drivers, transport
//! commands, credential helpers. A folder that ships its own `.git` directory
//! would otherwise run code as soon as Sage refreshes status, so Sage refuses
//! to run Git for a root whose repository config sets any of them.
//!
//! The verdict is cached per root against the exact bytes of every file that
//! decides which config Git reads. Each call re-reads those few small files
//! in-process and compares them, so steady-state polling starts no process,
//! and Git itself stays the only config parser.

use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    fs,
    io::{self, Read as _},
    path::{Component, Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
};

use crate::{
    error::{AppError, AppResult},
    git_budget,
};

use super::{GitOutput, GitProcessPolicy, GitRootAuthority};

const MAX_LOCATION_BYTES: usize = 64 * 1024;
const MAX_CONFIG_LIST_BYTES: usize = 1024 * 1024;
/// Larger watched files are refused rather than read on every call.
const MAX_WATCHED_FILE_BYTES: u64 = 256 * 1024;
const MAX_NAMED_KEYS: usize = 3;
const MAX_SUBSECTION_CHARS: usize = 64;
/// Idle entries are dropped once this many roots are cached.
const MAX_CACHED_ROOTS: usize = 256;

type AdmissionSlot = Arc<tokio::sync::Mutex<Option<Admission>>>;

static ADMISSIONS: LazyLock<Mutex<HashMap<PathBuf, AdmissionSlot>>> = LazyLock::new(Mutex::default);

/// The repository admission inspected. Admitted Git calls run with these
/// explicit locations, so they open exactly this repository.
///
/// A location inside the root is relative: the child's working directory is
/// the identity-checked root handle, so a directory swapped in at the root's
/// name cannot redirect it. A location outside the root (a linked worktree's
/// git dir, or the top level above a nested root) is absolute, so moving the
/// root elsewhere cannot redirect it either. Renaming one of those outside
/// directories also renames the root's path, which the spawn-time identity
/// check refuses.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct AdmittedRepository {
    git_dir: PathBuf,
    work_tree: PathBuf,
}

impl AdmittedRepository {
    fn pinned_from(root: &Path, git_dir: &Path, work_tree: &Path) -> AppResult<Self> {
        let root = root.canonicalize()?;
        let pin = |target: &Path| -> AppResult<PathBuf> {
            let target = target.canonicalize()?;
            Ok(match target.strip_prefix(&root) {
                Ok(inside) if inside.as_os_str().is_empty() => {
                    PathBuf::from(Component::CurDir.as_os_str())
                }
                Ok(inside) => inside.to_path_buf(),
                Err(_) => target,
            })
        };
        Ok(Self {
            git_dir: pin(git_dir)?,
            work_tree: pin(work_tree)?,
        })
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
    if let Some(admission) = cached.as_ref().filter(|admission| admission.is_current()) {
        return admission.verdict();
    }
    let probed = Probe::run(root, authority).await?;
    let verdict = probed.admission.verdict();
    *cached = probed.cacheable.then_some(probed.admission);
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
    })));
    ADMISSIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(root, slot);
}

struct Admission {
    watched: Vec<(PathBuf, Fingerprint)>,
    refusal: Option<String>,
    repository: Option<AdmittedRepository>,
}

impl Admission {
    fn is_current(&self) -> bool {
        self.watched
            .iter()
            .all(|(path, fingerprint)| Fingerprint::of(path) == *fingerprint)
    }

    fn verdict(&self) -> AppResult<Option<AdmittedRepository>> {
        match &self.refusal {
            Some(message) => Err(AppError::UnsafeRepositoryConfig(message.clone())),
            None => Ok(self.repository.clone()),
        }
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
            .map(|directory| watch(directory.join(".git")))
            .collect::<Vec<_>>();
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
                },
                cacheable,
            });
        }
        let Location {
            git_dir,
            common_dir,
            work_tree,
        } = Location::parse(&location)?;
        let (mut watched, cacheable) = discovery_path(ancestors, &git_dir);
        let repository = AdmittedRepository::pinned_from(root, &git_dir, &work_tree)?;
        watched.extend(
            [
                common_dir.join("config"),
                git_dir.join("config.worktree"),
                git_dir.join("commondir"),
            ]
            .into_iter()
            .map(watch),
        );
        let refusal = if watched
            .iter()
            .any(|(_, fingerprint)| fingerprint.is_unverifiable())
        {
            Some(TOO_LARGE.to_owned())
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
                return Err(AppError::Git(
                    String::from_utf8_lossy(&listing.stderr).trim().to_owned(),
                ));
            }
            if listing.stdout_truncated {
                Some(TOO_LARGE.to_owned())
            } else {
                refusal_message(&listing.stdout)
            }
        };
        Ok(Self {
            admission: Admission {
                watched,
                refusal,
                repository: Some(repository),
            },
            cacheable,
        })
    }
}

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

fn watch(path: PathBuf) -> (PathBuf, Fingerprint) {
    let fingerprint = Fingerprint::of(&path);
    (path, fingerprint)
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

/// What a watched path held when the verdict was reached. Regular files are
/// compared by their bytes, which no timestamp or size trick can forge.
/// Directories record only their kind: the files inside them that matter are
/// watched on their own, and a directory's timestamps move on every index or
/// ref update.
#[derive(Debug, PartialEq, Eq)]
enum Fingerprint {
    Missing,
    Unreadable(io::ErrorKind),
    Directory,
    Contents(Vec<u8>),
    /// Too large to re-read on every call; such a config is refused.
    Oversized,
    /// A FIFO, device or socket, which Git could block on; refused.
    Special,
}

impl Fingerprint {
    fn of(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::Directory,
            Ok(metadata) if !metadata.is_file() => Self::Special,
            Ok(metadata) if metadata.len() > MAX_WATCHED_FILE_BYTES => Self::Oversized,
            Ok(_) => match read_regular_file(path) {
                Ok(fingerprint) => fingerprint,
                Err(error) => Self::Unreadable(error.kind()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.kind()),
        }
    }

    fn is_unverifiable(&self) -> bool {
        matches!(self, Self::Oversized | Self::Special)
    }
}

/// Reads a file that was regular when checked. The handle is checked again so
/// a swap to a FIFO cannot block the caller.
fn read_regular_file(path: &Path) -> io::Result<Fingerprint> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(Fingerprint::Special);
    }
    let mut bytes = Vec::new();
    file.take(MAX_WATCHED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WATCHED_FILE_BYTES {
        return Ok(Fingerprint::Oversized);
    }
    Ok(Fingerprint::Contents(bytes))
}

/// Builds the public refusal for a NUL-separated `git config --name-only`
/// listing, or `None` when no listed key runs a command. Keys are classified
/// as bytes: Git accepts subsections that are not UTF-8.
fn refusal_message(listing: &[u8]) -> Option<String> {
    let keys = listing
        .split(|byte| *byte == 0)
        .filter_map(ConfigKey::parse)
        .filter(ConfigKey::runs_command)
        .map(|key| key.display_name())
        .collect::<BTreeSet<_>>();
    if keys.is_empty() {
        return None;
    }
    let mut named = keys
        .iter()
        .take(MAX_NAMED_KEYS)
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(", ");
    let remaining = keys.len().saturating_sub(MAX_NAMED_KEYS);
    if remaining > 0 {
        let _ = write!(named, " and {remaining} more");
    }
    Some(format!(
        "Sage did not run Git here because this repository's Git config sets keys that run \
         programs ({named}). Remove them from the repository config, or use the terminal for \
         Git in this folder."
    ))
}

/// A `section[.subsection].name` key as `git config --name-only` prints it.
#[derive(Debug, PartialEq, Eq)]
struct ConfigKey<'a> {
    section: Vec<u8>,
    subsection: Option<&'a [u8]>,
    name: Vec<u8>,
}

impl<'a> ConfigKey<'a> {
    fn parse(key: &'a [u8]) -> Option<Self> {
        let first = key.iter().position(|byte| *byte == b'.')?;
        let last = key.iter().rposition(|byte| *byte == b'.')?;
        let section = &key[..first];
        let name = &key[last + 1..];
        let subsection = (last > first).then(|| &key[first + 1..last]);
        (!section.is_empty() && !name.is_empty()).then(|| Self {
            section: section.to_ascii_lowercase(),
            subsection,
            name: name.to_ascii_lowercase(),
        })
    }

    /// Keys whose value Git runs as a program, plus includes, whose target
    /// files and conditions this check does not follow. Keys Sage overrides
    /// on every invocation (`core.hooksPath`, `core.fsmonitor`) are absent.
    fn runs_command(&self) -> bool {
        let scoped = self.subsection.is_some();
        match (self.section.as_slice(), self.name.as_slice()) {
            (b"include", _) | (b"includeif", _) => true,
            (b"filter", b"clean" | b"smudge" | b"process") => scoped,
            (b"diff", b"textconv" | b"command") => scoped,
            (b"diff", b"external") => !scoped,
            (b"merge", b"driver") => scoped,
            (
                b"core",
                b"sshcommand"
                | b"askpass"
                | b"gitproxy"
                | b"alternaterefscommand"
                | b"editor"
                | b"pager",
            ) => !scoped,
            (b"credential", b"helper") => true,
            (b"remote", b"uploadpack" | b"receivepack") => scoped,
            (b"sequence", b"editor") => !scoped,
            (b"gpg", b"program" | b"defaultkeycommand") => true,
            _ => false,
        }
    }

    /// The key as shown to the user. Subsections are user-controlled (and a
    /// credential subsection can be a URL with a token), so only plain
    /// identifiers are echoed.
    fn display_name(&self) -> String {
        let section = String::from_utf8_lossy(&self.section);
        let name = String::from_utf8_lossy(&self.name);
        match self.subsection.and_then(plain_identifier) {
            Some(subsection) => format!("{section}.{subsection}.{name}"),
            None if self.subsection.is_some() => format!("{section}.*.{name}"),
            None => format!("{section}.{name}"),
        }
    }
}

fn plain_identifier(bytes: &[u8]) -> Option<&str> {
    let plain = !bytes.is_empty()
        && bytes.len() <= MAX_SUBSECTION_CHARS
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'));
    plain.then(|| std::str::from_utf8(bytes).ok()).flatten()
}

#[cfg(test)]
mod tests;
