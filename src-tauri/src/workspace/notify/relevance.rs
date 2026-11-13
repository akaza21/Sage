//! What each watcher record means to the WebView. One decision per record,
//! made on the watcher thread before anything is queued, for every
//! platform's watcher alike.
//!
//! - A record under `.git` or `node_modules` is dropped, as the workspace
//!   walks skip them. Relevant `info/exclude` changes resync the root; other
//!   Git files are not needed: the WebView watches
//!   Git's change stamp (HEAD, the index and refs) itself, and Explorer never
//!   lists `.git`. `npm install` writes under `node_modules` by the thousand.
//! - A record under a folder that Git ignores (`.gitignore`, the repository's
//!   `info/exclude`, the runner's configured excludes file; not `.ignore`, which
//!   Git does not read) is folded into one record for the topmost ignored
//!   folder, marked [`Relevance::IgnoredBelow`]. Explorer still refreshes an
//!   expanded `target` or `dist` from it, at a throttled pace, while a
//!   `cargo build` no longer sends each of its writes. Git relevance comes
//!   from cached index membership; Search still uses its settings.
//!   The ignored folder's own record (`target` created at the root,
//!   say) passes as usual.
//! - Everything else passes.
//!
//! Root rules are loaded once and compact folder decisions are cached;
//! descendant matchers are rebuilt only on cache misses. A change
//! that can alter a decision (a `.gitignore`, any repository's
//! `info/exclude`, a repository appearing or going, a cached folder created,
//! removed or renamed, or lost records) drops the rules and the cache, so the next record
//! reloads them. Such a change inside a dropped or ignored folder alters
//! nothing outside it and is not counted. Externally resolved rule sources
//! and the runner's configuration are fingerprinted before cached decisions
//! are reused; unknown freshness always retains Git notifications.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::{
    git_process::IgnoreConfig,
    workspace::{
        directory_skip::skip_workspace_directory_name,
        ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
    },
};

/// Only compact decisions are cached, never descendant folder matchers.
/// Bound both entry overhead and the aggregate path bytes per watched root.
const MAX_CACHED_FOLDERS: usize = 4_096;
const MAX_CACHED_FOLDER_BYTES: usize = 1024 * 1024;

const REPOSITORY_EXCLUDE: &str = ".git/info/exclude";

/// What one record means to the WebView.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Relevance {
    /// Deliver the record as it is.
    Keep,
    /// Deliver nothing.
    Drop,
    /// Exclusion rules changed outside any skipped or ignored folder.
    Resync,
    /// Something changed inside this ignored folder, the topmost one above
    /// the record.
    IgnoredBelow(String),
}

/// The relevance filter of one watched root.
pub(super) struct NoticeRelevance {
    root: PathBuf,
    /// None is unprobed; Some(None) retains notifications after a failed probe.
    configuration: Option<Option<IgnoreConfig>>,
    /// Only global and root matchers survive a record, a fixed number of
    /// individually size-capped rule files, independent of the folder count.
    rules: Option<(IgnoreRules, FolderChain)>,
    /// Whether Git ignores each decided folder. Descendant matchers live only
    /// while deciding a cache miss, so siblings cannot retain rule chains.
    folders: HashMap<String, bool>,
    folder_bytes: usize,
    tracked: super::tracked::TrackedPaths,
}

impl NoticeRelevance {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            configuration: None,
            rules: None,
            folders: HashMap::new(),
            folder_bytes: 0,
            tracked: super::tracked::TrackedPaths::default(),
        }
    }

    /// What the record for `relative` (a root-relative path with `/`
    /// separators) with the watcher's `action` means to the WebView.
    pub(super) fn classify(&mut self, relative: &str, action: u32) -> Relevance {
        if self
            .rules
            .as_ref()
            .is_some_and(|(rules, _)| !rules.sources_current())
        {
            self.forget();
        }
        if let Some(repository) = repository_of_exclude(relative) {
            if !skipped(repository) && self.ignored_folder(repository).is_none() {
                self.forget();
                return Relevance::Resync;
            }
            return Relevance::Drop;
        }
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        if skipped(parent) {
            return Relevance::Drop;
        }
        if let Some(ignored) = self.ignored_folder(parent) {
            return Relevance::IgnoredBelow(ignored.to_string());
        }
        // A folder's own "changed" record says only that an entry inside it
        // came or went (Windows reports one for every parent on each create
        // or delete), which alters no decision; its creation, removal or
        // rename can, as can any change to a `.gitignore`.
        let moved = matches!(super::kind_for_action(action), Some("created" | "deleted"));
        if is_named(relative, ".gitignore")
            || moved && (is_named(relative, ".git") || self.folders.contains_key(relative))
        {
            self.forget();
        }
        Relevance::Keep
    }

    /// Drops the rules and every cached decision; the next record reloads
    /// them. Lost records may have hidden an ignore-file change.
    pub(super) fn forget(&mut self) {
        self.rules = None;
        self.folders.clear();
        self.folder_bytes = 0;
    }

    /// Decide both consumers before folding away the exact changed path.
    pub(super) fn classify_record(&mut self, relative: &str, action: u32) -> (Relevance, bool) {
        if self
            .configuration
            .as_ref()
            .and_then(Option::as_ref)
            .is_some_and(|config| !config.is_current())
        {
            self.configuration = None;
            self.forget();
        }
        if self.configuration.is_none() {
            self.configuration = Some(tauri::async_runtime::block_on(IgnoreConfig::load(
                &self.root,
            )));
        }
        // A removed directory is no longer on disk, and classifying its move
        // may clear folder decisions. Retain that bounded directory evidence.
        let known_directory = self.folders.contains_key(relative);
        let relevance = self.classify(relative, action);
        let git_relevant = match &relevance {
            Relevance::Keep | Relevance::IgnoredBelow(_) => {
                self.git_relevant(relative, action, known_directory)
            }
            Relevance::Drop | Relevance::Resync => true,
        };
        (relevance, git_relevant)
    }

    fn git_relevant(&mut self, relative: &str, action: u32, known_directory: bool) -> bool {
        if self
            .configuration
            .as_ref()
            .and_then(Option::as_ref)
            .is_none()
        {
            return true;
        }
        let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
        let ignored = self.ignored_folder(parent).or_else(|| {
            // Include the folder's own entry churn and clean/recreate events,
            // without mistaking an ordinary file for a directory-only rule.
            self.root
                .join(relative)
                .symlink_metadata()
                .map_or(known_directory, |metadata| metadata.is_dir())
                .then(|| self.ignored_folder(relative))
                .flatten()
        });
        if !self.sources_current() {
            return true;
        }
        let relevant = ignored.is_none_or(|folder| {
            self.tracked.relevant(
                &self.root,
                folder,
                relative,
                matches!(super::kind_for_action(action), Some("created" | "deleted")),
            )
        });
        relevant || !self.sources_current()
    }

    fn sources_current(&self) -> bool {
        self.configuration
            .as_ref()
            .and_then(Option::as_ref)
            .is_some_and(IgnoreConfig::is_current)
            && self
                .rules
                .as_ref()
                .is_none_or(|(rules, _)| rules.every_source_honoured() && rules.sources_current())
    }

    /// The topmost folder at or above `folder` that Git ignores, deciding
    /// from the top down.
    fn ignored_folder<'a>(&mut self, folder: &'a str) -> Option<&'a str> {
        if folder.is_empty() {
            return None;
        }
        let ends = || {
            folder
                .match_indices('/')
                .map(|(index, _)| index)
                .chain(std::iter::once(folder.len()))
        };
        let mut cached = true;
        for end in ends() {
            match self.folders.get(&folder[..end]) {
                Some(true) => return Some(&folder[..end]),
                Some(false) => {}
                None => cached = false,
            }
        }
        if cached {
            return None;
        }
        if self.folders.len() >= MAX_CACHED_FOLDERS
            || self.folder_bytes + folder.len() > MAX_CACHED_FOLDER_BYTES
        {
            self.folders.clear();
            self.folder_bytes = 0;
        }
        let (rules, root_chain) = self.rules.get_or_insert_with(|| {
            let excludes = self
                .configuration
                .as_ref()
                .and_then(Option::as_ref)
                .and_then(|config| config.excludes.as_deref());
            let rules = IgnoreRules::load(&self.root, true, excludes);
            let chain = rules.enter(
                &FolderChain::default(),
                "",
                RuleFiles::probe(&self.root).git_only(),
            );
            (rules, chain)
        });
        let root = &self.root;
        let folders = &mut self.folders;
        let folder_bytes = &mut self.folder_bytes;
        let git_rules = |path: &Path| RuleFiles::probe(path).git_only();
        let mut chain = root_chain.clone();
        for end in ends() {
            let prefix = &folder[..end];
            let ignored = match folders.get(prefix) {
                Some(ignored) => *ignored,
                None => {
                    let ignored = rules.is_ignored(&chain, prefix, true);
                    // Never evict mid-chain: retained descendants must keep
                    // their ancestor keys so moving an ancestor invalidates them.
                    if folders.len() < MAX_CACHED_FOLDERS
                        && *folder_bytes + prefix.len() <= MAX_CACHED_FOLDER_BYTES
                    {
                        folders.insert(prefix.to_string(), ignored);
                        *folder_bytes += prefix.len();
                    }
                    ignored
                }
            };
            if ignored {
                return Some(prefix);
            }
            if end < folder.len() {
                chain = rules.enter(&chain, prefix, git_rules(&root.join(prefix)));
            }
        }
        None
    }
}

/// Whether `folder` or a folder above it is `.git` or `node_modules`.
fn skipped(folder: &str) -> bool {
    !folder.is_empty()
        && folder
            .split('/')
            .any(|name| skip_workspace_directory_name(OsStr::new(name)))
}

/// The folder holding the repository whose `info/exclude` `relative` names
/// (empty for the root's own).
fn repository_of_exclude(relative: &str) -> Option<&str> {
    let split = relative.len().checked_sub(REPOSITORY_EXCLUDE.len())?;
    if !relative.is_char_boundary(split)
        || !relative[split..].eq_ignore_ascii_case(REPOSITORY_EXCLUDE)
    {
        return None;
    }
    match &relative[..split] {
        "" => Some(""),
        prefix => prefix.strip_suffix('/'),
    }
}

/// Whether the last name of `relative` is `name`, in any case.
fn is_named(relative: &str, name: &str) -> bool {
    relative
        .rsplit('/')
        .next()
        .is_some_and(|last| last.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests;
