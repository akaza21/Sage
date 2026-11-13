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
//!   `info/exclude`, the global excludes file; not ripgrep's `.ignore`, which
//!   Git does not read) is folded into one record for the topmost ignored
//!   folder, marked [`Relevance::IgnoredBelow`]. Explorer still refreshes an
//!   expanded `target` or `dist` from it, at a throttled pace, while a
//!   `cargo build` no longer sends each of its writes. Throttled notices also
//!   refresh Git (tracked descendants can be ignored) and Search using its
//!   settings. The ignored folder's own record (`target` created at the root,
//!   say) passes as usual.
//! - Everything else passes.
//!
//! Root rules are loaded once and compact folder decisions are cached;
//! descendant matchers are rebuilt only on cache misses. A change
//! that can alter a decision (a `.gitignore`, any repository's
//! `info/exclude`, a repository appearing or going, a cached folder created,
//! removed or renamed, or lost records) drops the rules and the cache, so the next record
//! reloads them. Such a change inside a dropped or ignored folder alters
//! nothing outside it and is not counted.

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
    global_excludes: Option<PathBuf>,
    /// Only global and root matchers survive a record, a fixed number of
    /// individually size-capped rule files, independent of the folder count.
    rules: Option<(IgnoreRules, FolderChain)>,
    /// Whether Git ignores each decided folder. Descendant matchers live only
    /// while deciding a cache miss, so siblings cannot retain rule chains.
    folders: HashMap<String, bool>,
    folder_bytes: usize,
}

impl NoticeRelevance {
    pub(super) fn new(root: &Path, global_excludes: Option<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            global_excludes,
            rules: None,
            folders: HashMap::new(),
            folder_bytes: 0,
        }
    }

    /// What the record for `relative` (a root-relative path with `/`
    /// separators) with the watcher's `action` means to the WebView.
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

    /// Drops the rules and every cached decision; the next record reloads
    /// them. Lost records may have hidden an ignore-file change.
    pub(super) fn forget(&mut self) {
        self.rules = None;
        self.folders.clear();
        self.folder_bytes = 0;
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
            let rules = IgnoreRules::load(&self.root, true, self.global_excludes.as_deref());
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
mod tests {
    use super::*;
    use std::fs;

    struct Root(PathBuf);

    impl Root {
        fn new() -> Self {
            let path = std::env::temp_dir()
                .join(format!("sage-notify-relevance-{}", uuid::Uuid::new_v4()));
            fs::create_dir_all(&path).unwrap();
            Self(path)
        }

        fn write(&self, relative: &str, contents: &str) {
            let path = self.0.join(relative);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, contents).unwrap();
        }
    }

    impl Drop for Root {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    const CHANGED: u32 = 3;
    const RENAMED_OLD: u32 = 4;

    fn ignored_below(folder: &str) -> Relevance {
        Relevance::IgnoredBelow(folder.to_string())
    }

    #[test]
    fn drops_skipped_folders_and_folds_ignored_ones_into_their_top_folder() {
        let root = Root::new();
        root.write(".gitignore", "target/\n*.log\n");
        root.write("target/debug/deps/app.d", "");
        root.write("src/nested/.gitignore", "generated/\n");
        root.write("tools/.ignore", "fixtures/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);

        for (relative, expected) in [
            ("node_modules/react/index.js", Relevance::Drop),
            ("packages/web/node_modules/a.js", Relevance::Drop),
            (".git/objects/ab/cdef", Relevance::Drop),
            ("target/debug/deps/app.d", ignored_below("target")),
            (
                "target/debug/.fingerprint/app-1/out",
                ignored_below("target"),
            ),
            ("target/node_modules/x.js", Relevance::Drop),
            (
                "src/nested/generated/out.rs",
                ignored_below("src/nested/generated"),
            ),
            ("target", Relevance::Keep),
            ("src/main.rs", Relevance::Keep),
            ("src/nested/lib.rs", Relevance::Keep),
            ("build.log", Relevance::Keep),
            (".git", Relevance::Keep),
            ("tools/fixtures/case.json", Relevance::Keep),
        ] {
            assert_eq!(
                relevance.classify(relative, CHANGED),
                expected,
                "{relative}"
            );
        }
    }

    #[test]
    fn a_gitignore_change_updates_the_filter() {
        let root = Root::new();
        root.write(".gitignore", "dist/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert_eq!(
            relevance.classify("dist/app.js", CHANGED),
            ignored_below("dist")
        );
        assert_eq!(relevance.classify("out/app.js", CHANGED), Relevance::Keep);

        root.write(".gitignore", "out/\n");
        assert_eq!(relevance.classify(".gitignore", CHANGED), Relevance::Keep);
        assert_eq!(relevance.classify("dist/app.js", CHANGED), Relevance::Keep);
        assert_eq!(
            relevance.classify("out/app.js", CHANGED),
            ignored_below("out")
        );
    }

    #[test]
    fn rule_files_inside_dropped_or_ignored_folders_keep_the_cache() {
        let root = Root::new();
        root.write(".gitignore", "target/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert_eq!(
            relevance.classify("target/a", CHANGED),
            ignored_below("target")
        );

        // Unignoring `target` would show up only after a reload.
        root.write(".gitignore", "");
        for relative in [
            "node_modules/pkg/.gitignore",
            "target/vendor/.gitignore",
            "target/vendor/.git",
            "node_modules/pkg/.git/info/exclude",
            "target/sub/.git/info/exclude",
        ] {
            assert_ne!(
                relevance.classify(relative, RENAMED_OLD),
                Relevance::Keep,
                "{relative}"
            );
        }
        assert_eq!(
            relevance.classify("target/a", CHANGED),
            ignored_below("target")
        );
    }

    #[test]
    fn a_repository_exclude_change_resyncs_consumers_and_updates_the_filter() {
        let root = Root::new();
        root.write(".git/info/exclude", "");
        root.write("sub/.git/info/exclude", "");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert_eq!(
            relevance.classify("scratch/notes.md", CHANGED),
            Relevance::Keep
        );
        assert_eq!(
            relevance.classify("sub/cache/blob", CHANGED),
            Relevance::Keep
        );

        root.write(".git/info/exclude", "scratch/\n");
        assert_eq!(
            relevance.classify(".git/info/exclude", CHANGED),
            Relevance::Resync
        );
        assert_eq!(
            relevance.classify("scratch/notes.md", CHANGED),
            ignored_below("scratch")
        );

        root.write("sub/.git/info/exclude", "cache/\n");
        assert_eq!(
            relevance.classify("sub/.git/info/exclude", CHANGED),
            Relevance::Resync
        );
        assert_eq!(
            relevance.classify("sub/cache/blob", CHANGED),
            ignored_below("sub/cache")
        );
    }

    #[test]
    fn bounded_cache_keeps_decisions_correct_when_an_ancestor_moves() {
        let root = Root::new();
        root.write(".gitignore", "target/\n");
        root.write("src/.gitignore", "generated/\n!generated-source/\n");
        root.write("lib/.gitignore", "cache/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        for index in 0..MAX_CACHED_FOLDERS - 1 {
            assert_eq!(
                relevance.classify(&format!("sibling-{index}/file"), CHANGED),
                Relevance::Keep
            );
        }
        assert_eq!(
            relevance.classify("lib/cache/blob", CHANGED),
            ignored_below("lib/cache")
        );
        assert!(relevance.folders.len() <= MAX_CACHED_FOLDERS);
        assert!(relevance.folder_bytes <= MAX_CACHED_FOLDER_BYTES);
        fs::remove_file(root.0.join("lib/.gitignore")).unwrap();
        assert_eq!(relevance.classify("lib", RENAMED_OLD), Relevance::Keep);
        assert_eq!(
            relevance.classify("lib/cache/blob", CHANGED),
            Relevance::Keep
        );
        for (path, expected) in [
            ("target/debug/out", ignored_below("target")),
            ("src/generated/out", ignored_below("src/generated")),
            ("src/generated-source/main.rs", Relevance::Keep),
            ("sibling-0/file", Relevance::Keep),
        ] {
            assert_eq!(relevance.classify(path, CHANGED), expected, "{path}");
        }
    }

    #[test]
    fn a_moved_folder_is_decided_again() {
        let root = Root::new();
        root.write("lib/.gitignore", "cache/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert_eq!(
            relevance.classify("lib/cache/blob", CHANGED),
            ignored_below("lib/cache")
        );

        // Windows reports a changed parent for every entry created inside it.
        fs::remove_file(root.0.join("lib/.gitignore")).unwrap();
        assert_eq!(relevance.classify("lib", CHANGED), Relevance::Keep);
        assert_eq!(
            relevance.classify("lib/cache/blob", CHANGED),
            ignored_below("lib/cache")
        );

        assert_eq!(relevance.classify("lib", RENAMED_OLD), Relevance::Keep);
        assert_eq!(
            relevance.classify("lib/cache/blob", CHANGED),
            Relevance::Keep
        );
    }
}
