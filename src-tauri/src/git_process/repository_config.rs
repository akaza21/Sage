//! Admission check for repository-local Git configuration.
//!
//! Sage clears the global and system Git config, but Git still honours the
//! repository's own `config` and `config.worktree`. Several keys there name a
//! program Git runs: content filters, diff and merge drivers, transport
//! commands, credential helpers. A folder that ships its own `.git` directory
//! would otherwise run code as soon as Sage refreshes status, so Sage refuses
//! to run Git for a root whose repository config sets any of them.
//!
//! The verdict is cached per root against the metadata of every file that can
//! change it. Steady-state polling costs a few `stat` calls and no process.

use std::{
    collections::{BTreeSet, HashMap},
    fmt::Write as _,
    fs, io,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock, Mutex, PoisonError},
    time::SystemTime,
};

use crate::{
    error::{AppError, AppResult},
    git_budget,
};

use super::{GitOutput, GitProcessPolicy, GitRootAuthority};

const MAX_LOCATION_BYTES: usize = 64 * 1024;
const MAX_CONFIG_LIST_BYTES: usize = 1024 * 1024;
const MAX_NAMED_KEYS: usize = 3;
const MAX_SUBSECTION_CHARS: usize = 64;

type AdmissionSlot = Arc<tokio::sync::Mutex<Option<Admission>>>;

static ADMISSIONS: LazyLock<Mutex<HashMap<PathBuf, AdmissionSlot>>> = LazyLock::new(Mutex::default);

/// Fails with [`AppError::UnsafeRepositoryConfig`] when the repository that
/// Git would open at `root` configures a command-running key.
pub(super) async fn admit(root: &Path, authority: Option<&GitRootAuthority>) -> AppResult<()> {
    let slot = Arc::clone(
        ADMISSIONS
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .entry(root.to_path_buf())
            .or_default(),
    );
    // Holding the per-root lock makes concurrent first reads share one probe.
    let mut cached = slot.lock().await;
    let admission = match cached.take() {
        Some(admission) if admission.is_current() => admission,
        _ => Admission::probe(root, authority).await?,
    };
    let verdict = admission.verdict();
    *cached = Some(admission);
    verdict
}

/// Admits `root` without checking its config, for tests that use a repository
/// filter as a deterministic way to keep a Git process running.
#[cfg(test)]
pub(crate) fn trust_for_test(root: &Path) {
    let root = root.canonicalize().expect("trusted test root exists");
    let slot = AdmissionSlot::new(tokio::sync::Mutex::new(Some(Admission {
        watched: Vec::new(),
        refusal: None,
    })));
    ADMISSIONS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(root, slot);
}

struct Admission {
    watched: Vec<(PathBuf, Stamp)>,
    refusal: Option<String>,
}

impl Admission {
    async fn probe(root: &Path, authority: Option<&GitRootAuthority>) -> AppResult<Self> {
        #[cfg(test)]
        tests::record_probe(root);
        // Discovery entries are stamped before Git looks, so a `.git` that
        // appears mid-probe invalidates the result on the next call.
        let mut watched = stamp_all(discovery_entries(root));
        let location = probe_git(
            root,
            authority,
            &[
                "rev-parse",
                "--path-format=absolute",
                "--git-dir",
                "--git-common-dir",
            ],
            MAX_LOCATION_BYTES,
        )
        .await?;
        if !location.status.success() {
            // No repository, so Git reads no repository config here.
            return Ok(Self {
                watched,
                refusal: None,
            });
        }
        let (git_dir, common_dir) = parse_location(&location)?;
        watched.extend(stamp_all(config_files(&git_dir, &common_dir)));

        let listing = probe_git(
            root,
            authority,
            &["config", "--list", "--no-includes", "--name-only", "-z"],
            MAX_CONFIG_LIST_BYTES,
        )
        .await?;
        if !listing.status.success() {
            return Err(AppError::Git(
                String::from_utf8_lossy(&listing.stderr).trim().to_owned(),
            ));
        }
        let refusal = if listing.stdout_truncated {
            Some(
                "Sage did not run Git here because this repository's Git config is too large \
                 to check. Use the terminal for Git in this folder."
                    .to_owned(),
            )
        } else {
            refusal_message(&listing.stdout)
        };
        Ok(Self { watched, refusal })
    }

    fn is_current(&self) -> bool {
        self.watched
            .iter()
            .all(|(path, stamp)| Stamp::of(path) == *stamp)
    }

    fn verdict(&self) -> AppResult<()> {
        match &self.refusal {
            Some(message) => Err(AppError::UnsafeRepositoryConfig(message.clone())),
            None => Ok(()),
        }
    }
}

async fn probe_git(
    root: &Path,
    authority: Option<&GitRootAuthority>,
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
    )
    .await
}

/// Every `.git` entry Git's discovery could stop at, from `root` upwards.
fn discovery_entries(root: &Path) -> impl Iterator<Item = PathBuf> {
    root.ancestors().map(|directory| directory.join(".git"))
}

/// Every file that can change which repository config Git reads.
fn config_files(git_dir: &Path, common_dir: &Path) -> impl Iterator<Item = PathBuf> {
    [
        common_dir.join("config"),
        git_dir.join("config.worktree"),
        git_dir.join("commondir"),
    ]
    .into_iter()
}

fn stamp_all(paths: impl Iterator<Item = PathBuf>) -> Vec<(PathBuf, Stamp)> {
    paths
        .map(|path| {
            let stamp = Stamp::of(&path);
            (path, stamp)
        })
        .collect()
}

fn parse_location(output: &GitOutput) -> AppResult<(PathBuf, PathBuf)> {
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git directory path is not valid UTF-8".into()))?;
    let mut lines = text.lines();
    match (lines.next(), lines.next()) {
        (Some(git_dir), Some(common_dir)) if !git_dir.is_empty() && !common_dir.is_empty() => {
            Ok((PathBuf::from(git_dir), PathBuf::from(common_dir)))
        }
        _ => Err(AppError::Git(
            "Git did not report the repository directory".into(),
        )),
    }
}

/// What a path looked like when the verdict was reached. Directories record
/// only their kind: their contents that matter are watched separately, and
/// their timestamps move on every index or ref update.
#[derive(Debug, PartialEq, Eq)]
enum Stamp {
    Missing,
    Unreadable(io::ErrorKind),
    Directory,
    File(FileIdentity),
}

#[derive(Debug, PartialEq, Eq)]
struct FileIdentity {
    len: u64,
    modified: Option<SystemTime>,
    /// The inode and status-change time defeat a rewrite that restores the
    /// old size and modification time; neither can be set from user space.
    #[cfg(unix)]
    inode: (u64, u64),
    #[cfg(unix)]
    changed: (i64, i64),
    #[cfg(not(unix))]
    created: Option<SystemTime>,
}

impl Stamp {
    fn of(path: &Path) -> Self {
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::Directory,
            Ok(metadata) => Self::File(FileIdentity::of(&metadata)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.kind()),
        }
    }
}

impl FileIdentity {
    fn of(metadata: &fs::Metadata) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;

        Self {
            len: metadata.len(),
            modified: metadata.modified().ok(),
            #[cfg(unix)]
            inode: (metadata.dev(), metadata.ino()),
            #[cfg(unix)]
            changed: (metadata.ctime(), metadata.ctime_nsec()),
            #[cfg(not(unix))]
            created: metadata.created().ok(),
        }
    }
}

/// Builds the public refusal for a NUL-separated `git config --name-only`
/// listing, or `None` when no listed key runs a command.
fn refusal_message(listing: &[u8]) -> Option<String> {
    let keys = listing
        .split(|byte| *byte == 0)
        .filter_map(|key| std::str::from_utf8(key).ok())
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
    section: String,
    subsection: Option<&'a str>,
    name: String,
}

impl<'a> ConfigKey<'a> {
    fn parse(key: &'a str) -> Option<Self> {
        let (section, rest) = key.split_once('.')?;
        let (subsection, name) = match rest.rsplit_once('.') {
            Some((subsection, name)) => (Some(subsection), name),
            None => (None, rest),
        };
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
        match (self.section.as_str(), self.name.as_str()) {
            ("include", _) | ("includeif", _) => true,
            ("filter", "clean" | "smudge" | "process") => scoped,
            ("diff", "textconv" | "command") => scoped,
            ("diff", "external") => !scoped,
            ("merge", "driver") => scoped,
            (
                "core",
                "sshcommand" | "askpass" | "gitproxy" | "alternaterefscommand" | "editor" | "pager",
            ) => !scoped,
            ("credential", "helper") => true,
            ("remote", "uploadpack" | "receivepack") => scoped,
            ("sequence", "editor") => !scoped,
            ("gpg", "program" | "defaultkeycommand") => true,
            _ => false,
        }
    }

    /// The key as shown to the user. Subsections are user-controlled (and a
    /// credential subsection can be a URL with a token), so only plain
    /// identifiers are echoed.
    fn display_name(&self) -> String {
        match self.subsection {
            None => format!("{}.{}", self.section, self.name),
            Some(subsection) if is_plain_identifier(subsection) => {
                format!("{}.{subsection}.{}", self.section, self.name)
            }
            Some(_) => format!("{}.*.{}", self.section, self.name),
        }
    }
}

fn is_plain_identifier(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= MAX_SUBSECTION_CHARS
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
}

#[cfg(test)]
mod tests;
