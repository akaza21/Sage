//! Precedence follows ripgrep, which VS Code search runs: a `.ignore` match
//! beats a `.gitignore` match, and within each kind the deepest folder wins.
//! `.gitignore` files apply up to the nearest repository, then that
//! repository's `.git/info/exclude`, then the user's global excludes file.
//!
//! Only rule files inside the root count (VS Code's `search.useParentIgnoreFiles`
//! off), so a repository around the root, such as a home-folder dotfiles
//! repository ignoring `*`, cannot hide a Project. Rule files are opened
//! without following links, as Git 2.32 and later do.

use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};

mod repository;
#[cfg(test)]
use repository::git_target;
use repository::repository_exclude;

/// A larger rule file is skipped and logged, which bounds the time and
/// memory one file can cost a walk. Real ignore files are a few KiB.
const MAX_RULES_FILE_BYTES: u64 = 256 * 1024;
/// Matching follows the file system's usual case rule, as Git's
/// `core.ignoreCase` does.
const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuleFiles {
    ignore: bool,
    gitignore: bool,
    repository: bool,
}

impl RuleFiles {
    pub(crate) fn note(&mut self, name: &OsStr) {
        let Some(name) = name.to_str() else { return };
        let is = |expected: &str| {
            if CASE_INSENSITIVE {
                name.eq_ignore_ascii_case(expected)
            } else {
                name == expected
            }
        };
        self.ignore |= is(".ignore");
        self.gitignore |= is(".gitignore");
        self.repository |= is(".git");
    }

    pub(crate) fn git_only(self) -> Self {
        Self {
            ignore: false,
            ..self
        }
    }

    /// Only a missing name is absent: a name that cannot be inspected (access
    /// denied, say) counts as present, so reading it fails as a skipped source
    /// instead of being treated as no rules at all.
    pub(crate) fn probe(directory: &Path) -> Self {
        let exists = |name: &str| {
            !matches!(
                directory.join(name).symlink_metadata(),
                Err(error) if error.kind() == ErrorKind::NotFound
            )
        };
        Self {
            ignore: exists(".ignore"),
            gitignore: exists(".gitignore"),
            repository: exists(".git"),
        }
    }
}

struct FolderRules {
    /// Root-relative with a trailing `/`; empty for the root.
    prefix: String,
    ignore: Option<Gitignore>,
    gitignore: Option<Gitignore>,
    /// The folder holds a repository: `.gitignore` files above it do not
    /// apply inside it, and this is its exclude file.
    repository: Option<Option<Gitignore>>,
    parent: Option<Arc<FolderRules>>,
}

#[derive(Clone, Default)]
pub(crate) struct FolderChain(Option<Arc<FolderRules>>);

impl FolderChain {
    fn folders(&self) -> impl Iterator<Item = &FolderRules> {
        std::iter::successors(self.0.as_deref(), |folder| folder.parent.as_deref())
    }
}

pub(crate) struct IgnoreRules {
    root: PathBuf,
    enabled: bool,
    global: Option<Gitignore>,
    skipped: AtomicBool,
}

struct Skipped;

impl IgnoreRules {
    /// `global_excludes` patterns match paths relative to the root.
    pub(crate) fn load(root: &Path, enabled: bool, global_excludes: Option<&Path>) -> Self {
        let mut rules = Self {
            root: root.to_path_buf(),
            enabled,
            global: None,
            skipped: AtomicBool::new(false),
        };
        rules.global = global_excludes
            .filter(|_| enabled)
            .filter(|path| path.is_absolute())
            .and_then(|path| rules.honour(read_rules(path, true)))
            .and_then(|bytes| matcher(&bytes));
        rules
    }

    /// Walkers that must not admit an entry a skipped source might have ignored,
    /// such as prompt attachments, check this after building each chain.
    pub(crate) fn every_source_honoured(&self) -> bool {
        !self.skipped.load(Ordering::Relaxed)
    }

    fn honour<T>(&self, source: Result<Option<T>, Skipped>) -> Option<T> {
        source.unwrap_or_else(|Skipped| {
            self.skipped.store(true, Ordering::Relaxed);
            None
        })
    }

    pub(crate) fn enter(
        &self,
        parent: &FolderChain,
        relative: &str,
        files: RuleFiles,
    ) -> FolderChain {
        if !self.enabled || files == RuleFiles::default() {
            return parent.clone();
        }
        let directory = self.root.join(relative);
        let read = |present: bool, name: &str| {
            present
                .then(|| self.honour(read_rules(&directory.join(name), false)))
                .flatten()
                .and_then(|bytes| matcher(&bytes))
        };
        let ignore = read(files.ignore, ".ignore");
        let gitignore = read(files.gitignore, ".gitignore");
        let repository = files
            .repository
            .then(|| self.honour(repository_exclude(self, &directory, relative.is_empty())));
        if ignore.is_none() && gitignore.is_none() && repository.is_none() {
            return parent.clone();
        }
        FolderChain(Some(Arc::new(FolderRules {
            prefix: if relative.is_empty() {
                String::new()
            } else {
                format!("{relative}/")
            },
            ignore,
            gitignore,
            repository,
            parent: parent.0.clone(),
        })))
    }

    /// Parent folders of `relative` are not checked.
    pub(crate) fn is_ignored(&self, chain: &FolderChain, relative: &str, is_dir: bool) -> bool {
        if !self.enabled {
            return false;
        }
        let decide = |matcher: Option<&Gitignore>, folder: &FolderRules| {
            let below = relative.strip_prefix(&folder.prefix).unwrap_or(relative);
            matcher.and_then(|matcher| decided(matcher.matched(below, is_dir)))
        };
        if let Some(ignored) = chain
            .folders()
            .find_map(|folder| decide(folder.ignore.as_ref(), folder))
        {
            return ignored;
        }
        let mut repository = None;
        for folder in chain.folders() {
            if let Some(ignored) = decide(folder.gitignore.as_ref(), folder) {
                return ignored;
            }
            if let Some(exclude) = &folder.repository {
                repository = Some((folder, exclude.as_ref()));
                break;
            }
        }
        if let Some(ignored) = repository
            .and_then(|(folder, exclude)| decide(exclude, folder))
            .or_else(|| {
                let global = self.global.as_ref()?;
                decided(global.matched(relative, is_dir))
            })
        {
            return ignored;
        }
        false
    }

    pub(crate) fn chain_for(&self, relative: &str, files: RuleFiles) -> (FolderChain, bool) {
        if !self.enabled {
            return (FolderChain::default(), false);
        }
        let components = relative
            .split('/')
            .filter(|part| !part.is_empty())
            .collect::<Vec<_>>();
        let files_of = |index: usize, path: &str| {
            if index == components.len() {
                files
            } else {
                RuleFiles::probe(&self.root.join(path))
            }
        };
        let mut chain = self.enter(&FolderChain::default(), "", files_of(0, ""));
        let mut path = String::new();
        for (index, component) in components.iter().enumerate() {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(component);
            if self.is_ignored(&chain, &path, true) {
                return (chain, true);
            }
            chain = self.enter(&chain, &path, files_of(index + 1, &path));
        }
        (chain, false)
    }
}

/// The search helper runs with an empty environment, so the app resolves this
/// for it. Read once per launch; a changed `core.excludesFile` applies after a
/// restart.
pub(crate) fn global_excludes_path() -> Option<PathBuf> {
    static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        ignore::gitignore::gitconfig_excludes_path()
            .map(|path| {
                // Tilde expansion can append Git's `/` to a verbatim Windows
                // home. There `/` is a literal, not a path separator.
                #[cfg(windows)]
                let path = {
                    use std::ffi::OsString;
                    use std::os::windows::ffi::{OsStrExt, OsStringExt};

                    let wide: Vec<u16> = path
                        .as_os_str()
                        .encode_wide()
                        .map(|unit| {
                            if unit == u16::from(b'/') {
                                u16::from(b'\\')
                            } else {
                                unit
                            }
                        })
                        .collect();
                    PathBuf::from(OsString::from_wide(&wide))
                };
                path.components().collect::<PathBuf>()
            })
            .filter(|path| path.is_absolute())
    })
    .clone()
}

/// Only `NotFound` means there is no repository. Any other inspection error
/// leaves its exclude rules unknown, which is a skipped source.
fn present(
    path: &Path,
    metadata: std::io::Result<std::fs::Metadata>,
) -> Result<Option<std::fs::Metadata>, Skipped> {
    match metadata {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(skipped(path, &error.to_string())),
    }
}

fn unresolved(path: &Path) -> Skipped {
    skipped(path, "Git directory reference not followed");
    Skipped
}

/// A missing file is `Ok(None)`; anything else unusable is logged and [`Skipped`].
fn read_rules(path: &Path, follow: bool) -> Result<Option<Vec<u8>>, Skipped> {
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        // Non-blocking, so a FIFO planted under a rule file's name cannot
        // stall the walk.
        let no_follow = if follow { 0 } else { libc::O_NOFOLLOW };
        options.custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC | no_follow);
    }
    #[cfg(windows)]
    if !follow {
        use std::os::windows::fs::OpenOptionsExt;
        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(skipped(path, &error.to_string())),
    };
    read_regular(file, path).map(Some)
}

fn read_regular(file: File, path: &Path) -> Result<Vec<u8>, Skipped> {
    let metadata = file
        .metadata()
        .map_err(|error| skipped(path, &error.to_string()))?;
    if !metadata.is_file() || metadata.file_type().is_symlink() {
        return Err(skipped(path, "not a regular file"));
    }
    let mut bytes = Vec::new();
    if let Err(error) = file.take(MAX_RULES_FILE_BYTES + 1).read_to_end(&mut bytes) {
        return Err(skipped(path, &error.to_string()));
    }
    if bytes.len() as u64 > MAX_RULES_FILE_BYTES {
        return Err(skipped(path, "larger than 256 KiB"));
    }
    Ok(bytes)
}

fn skipped(path: &Path, reason: &str) -> Skipped {
    tracing::warn!(path = %path.display(), reason, "ignore rule file skipped");
    Skipped
}

/// `None` when no pattern remains.
fn matcher(bytes: &[u8]) -> Option<Gitignore> {
    let text = String::from_utf8_lossy(bytes);
    // The root `.` makes the matcher take paths as given, relative to the
    // rule file's folder.
    let mut builder = GitignoreBuilder::new(".");
    builder.case_insensitive(CASE_INSENSITIVE).ok()?;
    for line in text.trim_start_matches('\u{feff}').lines() {
        let _ = builder.add_line(None, line);
    }
    builder.build().ok().filter(|matcher| !matcher.is_empty())
}

/// `Some(true)` ignores, `Some(false)` is a negated match, `None` is no match.
fn decided<T>(matched: Match<T>) -> Option<bool> {
    match matched {
        Match::None => None,
        Match::Ignore(_) => Some(true),
        Match::Whitelist(_) => Some(false),
    }
}

#[cfg(test)]
mod tests;
