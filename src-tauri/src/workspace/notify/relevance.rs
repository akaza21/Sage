use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::workspace::{
    directory_skip::skip_workspace_directory_name,
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
};

/// Only compact decisions are cached, never descendant folder matchers.
/// Bound both entry overhead and the aggregate path bytes per watched root.
const MAX_CACHED_FOLDERS: usize = 4_096;
const MAX_CACHED_FOLDER_BYTES: usize = 1024 * 1024;

const REPOSITORY_EXCLUDE: &str = ".git/info/exclude";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum Relevance {
    Keep,
    Drop,
    Resync,
    IgnoredBelow(String),
}

pub(super) struct NoticeRelevance {
    root: PathBuf,
    excludes: Option<PathBuf>,
    /// Only global and root matchers survive a record, a fixed number of
    /// individually size-capped rule files, independent of the folder count.
    rules: Option<(IgnoreRules, FolderChain)>,
    /// Descendant matchers live only while deciding a cache miss, so siblings
    /// cannot retain rule chains.
    folders: HashMap<String, bool>,
    folder_bytes: usize,
}

impl NoticeRelevance {
    pub(super) fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            excludes: None,
            rules: None,
            folders: HashMap::new(),
            folder_bytes: 0,
        }
    }

    pub(super) fn with_excludes(mut self, excludes: Option<PathBuf>) -> Self {
        self.excludes = excludes;
        self
    }

    pub(super) fn classify(&mut self, relative: &str, action: u32) -> Relevance {
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

    /// Lost records may have hidden an ignore-file change.
    pub(super) fn forget(&mut self) {
        self.rules = None;
        self.folders.clear();
        self.folder_bytes = 0;
    }

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
            let rules = IgnoreRules::load(&self.root, true, self.excludes.as_deref());
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

fn skipped(folder: &str) -> bool {
    !folder.is_empty()
        && folder
            .split('/')
            .any(|name| skip_workspace_directory_name(OsStr::new(name)))
}

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

fn is_named(relative: &str, name: &str) -> bool {
    relative
        .rsplit('/')
        .next()
        .is_some_and(|last| last.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests;
