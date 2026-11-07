//! Gitignore rules for workspace walks (EDIT-0005 AD-3). Sage keeps its own
//! walkers, which own containment, caps, sort order and continuation, and asks
//! these rules whether an entry is ignored.
//!
//! Precedence follows ripgrep, which VS Code search runs: a `.ignore` match
//! beats a `.gitignore` match, and within each kind the deepest folder wins.
//! `.gitignore` files apply up to the nearest repository, then that
//! repository's `.git/info/exclude`, then the user's global excludes file,
//! whose patterns match paths relative to the root. A
//! negated pattern (`!keep.log`) that matches first re-includes the entry. An
//! entry inside an ignored folder is ignored too, as in Git, which walkers get
//! by not entering ignored folders.
//!
//! Only rule files inside the root count, as with VS Code's default
//! (`search.useParentIgnoreFiles` off), so a repository around the root, such
//! as a home-folder dotfiles repository ignoring `*`, cannot hide a Project.
//! Rule files are opened without following links, as Git 2.32 and later do,
//! and each is read up to a size cap. Only patterns are read from them; nothing
//! is listed. A rule source that exists but cannot be honoured is skipped and
//! logged, which listings tolerate; [`IgnoreRules::every_source_honoured`]
//! lets callers that must fail closed refuse the walk instead.

use std::{
    ffi::OsStr,
    fs::{File, OpenOptions},
    io::{ErrorKind, Read},
    path::{Component, Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ignore::{
    Match,
    gitignore::{Gitignore, GitignoreBuilder},
};

/// A larger rule file is skipped and logged, which bounds the time and
/// memory one file can cost a walk. Real ignore files are a few KiB.
const MAX_RULES_FILE_BYTES: u64 = 256 * 1024;
/// Matching follows the file system's usual case rule, as Git's
/// `core.ignoreCase` does.
const CASE_INSENSITIVE: bool = cfg!(any(windows, target_os = "macos"));

/// The rule-bearing names a folder listing holds.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RuleFiles {
    ignore: bool,
    gitignore: bool,
    repository: bool,
}

impl RuleFiles {
    /// Records `name` if it is `.ignore`, `.gitignore` or `.git`.
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

    /// The rule-bearing names in `directory`, found without listing it.
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

/// One folder's own rules, linked to its parent folder's.
struct FolderRules {
    /// The folder relative to the root with a trailing `/`, or empty for the
    /// root itself.
    prefix: String,
    ignore: Option<Gitignore>,
    gitignore: Option<Gitignore>,
    /// The folder holds a repository: `.gitignore` files above it do not
    /// apply inside it, and this is its exclude file.
    repository: Option<Option<Gitignore>>,
    parent: Option<Arc<FolderRules>>,
}

/// The rules in force inside one folder. Folders without rule files share
/// their parent's chain, so a clone is one reference count.
#[derive(Clone, Default)]
pub(crate) struct FolderChain(Option<Arc<FolderRules>>);

impl FolderChain {
    fn folders(&self) -> impl Iterator<Item = &FolderRules> {
        std::iter::successors(self.0.as_deref(), |folder| folder.parent.as_deref())
    }
}

/// The rules for one walk of one root.
pub(crate) struct IgnoreRules {
    root: PathBuf,
    enabled: bool,
    global: Option<Gitignore>,
    /// Set once any resolved rule source was skipped.
    skipped: AtomicBool,
}

/// A rule source that exists but was skipped (already logged): over the cap,
/// a link where links are refused, not a regular file, unreadable, or a Git
/// directory reference that does not resolve to an admitted location.
struct Skipped;

impl IgnoreRules {
    /// Rules for walks of `root`. When `enabled` is false nothing is read and
    /// nothing is ignored. `global_excludes` is Git's `core.excludesFile`,
    /// resolved by the caller (see [`global_excludes_path`]); its patterns
    /// match paths relative to the root.
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

    /// Whether every rule source resolved so far (the global excludes file
    /// and the sources of every folder entered) was applied in full. Walkers
    /// that must not admit an entry a skipped source might have ignored, such
    /// as prompt attachments, check this after building each chain.
    pub(crate) fn every_source_honoured(&self) -> bool {
        !self.skipped.load(Ordering::Relaxed)
    }

    fn honour<T>(&self, source: Result<Option<T>, Skipped>) -> Option<T> {
        source.unwrap_or_else(|Skipped| {
            self.skipped.store(true, Ordering::Relaxed);
            None
        })
    }

    /// The chain for the folder at `relative` (empty for the root), whose
    /// listing held `files`, given its parent's chain.
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
        let repository = files.repository.then(|| {
            self.honour(repository_exclude(
                &self.root,
                &directory,
                relative.is_empty(),
            ))
        });
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

    /// Whether the entry at `relative` is ignored by the rules in force in
    /// its folder, whose chain is `chain`. Its parent folders are not checked.
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

    /// The chain inside the folder at `relative` (empty for the root), whose
    /// listing held `files`, and whether that folder or any folder above it
    /// is ignored. For listings of one folder, such as the Explorer's;
    /// walkers build chains as they go.
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

/// Git's `core.excludesFile`, from the user's Git configuration. The search
/// helper runs with an empty environment, so the app resolves this for it.
///
/// Read once per launch, so Explorer listings do not reread Git's
/// configuration files; a changed `core.excludesFile` applies after a restart.
pub(crate) fn global_excludes_path() -> Option<PathBuf> {
    static PATH: std::sync::OnceLock<Option<PathBuf>> = std::sync::OnceLock::new();
    PATH.get_or_init(|| {
        ignore::gitignore::gitconfig_excludes_path().filter(|path| path.is_absolute())
    })
    .clone()
}

/// The `info/exclude` matcher of the repository in `directory`. A `.git` file
/// (a worktree or submodule) names its Git directory, and a worktree's
/// `commondir` names the shared one that holds `info/exclude`. Only the
/// root's own `.git` file (a worktree opened as the Project) may lead outside
/// the root.
fn repository_exclude(
    root: &Path,
    directory: &Path,
    is_root: bool,
) -> Result<Option<Gitignore>, Skipped> {
    let dot_git = directory.join(".git");
    let Some(metadata) = present(&dot_git, dot_git.symlink_metadata())? else {
        return Ok(None);
    };
    let mut git_directory = if metadata.is_dir() {
        dot_git
    } else {
        let Some(link) = read_rules(&dot_git, false)? else {
            return Ok(None);
        };
        String::from_utf8(link)
            .ok()
            .and_then(|link| {
                let target = link.trim().strip_prefix("gitdir:")?;
                git_target(directory, target, root, is_root)
            })
            .ok_or_else(|| unresolved(&dot_git))?
    };
    let commondir = git_directory.join("commondir");
    if let Some(common) = read_rules(&commondir, true)? {
        git_directory = String::from_utf8(common)
            .ok()
            .and_then(|common| git_target(&git_directory, &common, root, is_root))
            .ok_or_else(|| unresolved(&commondir))?;
    }
    Ok(read_rules(&git_directory.join("info").join("exclude"), true)?.and_then(|b| matcher(&b)))
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

/// Where a `gitdir:` or `commondir` value written in `base` leads, resolved
/// without touching the file system. Network paths (`\\host\share`,
/// `//host`) are refused, and unless `outside_allowed` the result must stay
/// inside `root`, so a `.git` file in a cloned or unzipped folder cannot make
/// a listing contact another machine (an automounted `/net/host`, say).
fn git_target(base: &Path, text: &str, root: &Path, outside_allowed: bool) -> Option<PathBuf> {
    let text = text.trim();
    if text.is_empty() || text.starts_with("\\\\") || text.starts_with("//") {
        return None;
    }
    let path = Path::new(text);
    let relative = path.components().all(|component| {
        matches!(
            component,
            Component::Normal(_) | Component::CurDir | Component::ParentDir
        )
    });
    if !relative && !path.is_absolute() {
        return None;
    }
    #[cfg(windows)]
    if !relative
        && !matches!(
            path.components().next(),
            Some(Component::Prefix(prefix)) if matches!(prefix.kind(), std::path::Prefix::Disk(_))
        )
    {
        return None;
    }
    let mut target = PathBuf::new();
    for component in base.join(path).components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                target.pop();
            }
            other => target.push(other),
        }
    }
    (outside_allowed || target.starts_with(root)).then_some(target)
}

/// Reads a rule file up to the cap. A missing file is `Ok(None)`; a link when
/// `follow` is false, a non-file, an unreadable file or one over the cap is
/// logged and [`Skipped`].
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

/// A matcher for one rule file's lines. Patterns that do not parse are
/// skipped, as Git skips them. `None` when no pattern remains.
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

/// `Some(true)` for an ignore match, `Some(false)` for a negated pattern and
/// `None` when nothing matched.
fn decided<T>(matched: Match<T>) -> Option<bool> {
    match matched {
        Match::None => None,
        Match::Ignore(_) => Some(true),
        Match::Whitelist(_) => Some(false),
    }
}

#[cfg(test)]
mod tests;
