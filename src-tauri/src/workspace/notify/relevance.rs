//! Which watcher records may reach the WebView. One decision per record, made
//! on the watcher thread before anything is queued, for every platform's
//! watcher alike.
//!
//! A record is dropped when one of the folders above it is `.git`,
//! `node_modules` or ignored by the root's ignore files, as the workspace
//! walks skip them: a `cargo build` or `npm install` writes there by the
//! thousand, and neither the Explorer listing above that folder nor Git
//! status changes with those writes. The ignored folder's own record (the
//! `target` folder created at the root, say) still passes, so the listing
//! that shows it stays current. Git's own files are not needed either: the
//! WebView watches Git's change stamp (HEAD, the index and refs) itself and
//! ignores notices under `.git`.
//!
//! The rules are loaded once and each folder's decision is cached. A change
//! that can alter a decision (an ignore file, `.git/info/exclude`, a
//! repository appearing or going, a cached folder moving, or lost records)
//! drops the rules and the cache, so the next record reloads them.

use std::{
    collections::HashMap,
    ffi::OsStr,
    path::{Path, PathBuf},
};

use crate::workspace::{
    directory_skip::skip_workspace_directory_name,
    ignore_rules::{FolderChain, IgnoreRules, RuleFiles},
};

/// Folder decisions kept before the cache starts again. A build tree is
/// decided at its top folder, so real Projects stay far below this; the
/// bound only caps a pathological tree's memory (a few hundred KiB).
const MAX_CACHED_FOLDERS: usize = 4_096;

/// The relevance filter of one watched root.
pub(super) struct NoticeRelevance {
    root: PathBuf,
    global_excludes: Option<PathBuf>,
    rules: Option<IgnoreRules>,
    /// Each decided folder's rules, or `None` when the folder is excluded.
    /// The root is the empty key.
    folders: HashMap<String, Option<FolderChain>>,
}

impl NoticeRelevance {
    pub(super) fn new(root: &Path, global_excludes: Option<PathBuf>) -> Self {
        Self {
            root: root.to_path_buf(),
            global_excludes,
            rules: None,
            folders: HashMap::new(),
        }
    }

    /// Whether the record for `relative` (a root-relative path with `/`
    /// separators) should reach the WebView.
    pub(super) fn admits(&mut self, relative: &str) -> bool {
        if changes_rules(relative) || self.folders.contains_key(relative) {
            self.forget();
        }
        let Some((parent, _)) = relative.rsplit_once('/') else {
            return true;
        };
        if parent
            .split('/')
            .any(|name| skip_workspace_directory_name(OsStr::new(name)))
        {
            return false;
        }
        !self.folder_excluded(parent)
    }

    /// Drops the rules and every cached decision; the next record reloads
    /// them. Lost records may have hidden an ignore-file change.
    pub(super) fn forget(&mut self) {
        self.rules = None;
        self.folders.clear();
    }

    /// Whether `folder` or a folder above it is ignored, deciding from the
    /// top down and stopping at the first excluded folder.
    fn folder_excluded(&mut self, folder: &str) -> bool {
        if self.folders.len() >= MAX_CACHED_FOLDERS {
            self.folders.clear();
        }
        let rules = self.rules.get_or_insert_with(|| {
            IgnoreRules::load(&self.root, true, self.global_excludes.as_deref())
        });
        let root = &self.root;
        let folders = &mut self.folders;
        let mut chain = folders
            .entry(String::new())
            .or_insert_with(|| {
                Some(rules.enter(&FolderChain::default(), "", RuleFiles::probe(root)))
            })
            .clone()
            .unwrap_or_default();
        let ends = folder
            .match_indices('/')
            .map(|(index, _)| index)
            .chain(std::iter::once(folder.len()));
        for end in ends {
            let prefix = &folder[..end];
            let decided = match folders.get(prefix) {
                Some(decided) => decided.clone(),
                None => {
                    let decided = (!rules.is_ignored(&chain, prefix, true))
                        .then(|| rules.enter(&chain, prefix, RuleFiles::probe(&root.join(prefix))));
                    folders.insert(prefix.to_string(), decided.clone());
                    decided
                }
            };
            match decided {
                Some(next) => chain = next,
                None => return true,
            }
        }
        false
    }
}

/// Whether a record for `relative` can change what the ignore rules decide.
fn changes_rules(relative: &str) -> bool {
    let name = relative.rsplit('/').next().unwrap_or(relative);
    [".gitignore", ".ignore", ".git"]
        .iter()
        .any(|rule| name.eq_ignore_ascii_case(rule))
        || relative.eq_ignore_ascii_case(".git/info/exclude")
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

    #[test]
    fn drops_records_under_skipped_and_ignored_folders_and_keeps_the_rest() {
        let root = Root::new();
        root.write(".gitignore", "target/\n*.log\n");
        root.write("target/debug/deps/app.d", "");
        root.write("src/nested/.gitignore", "generated/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);

        for dropped in [
            "target/debug/deps/app.d",
            "target/debug/.fingerprint/app-1/output",
            "node_modules/react/index.js",
            "packages/web/node_modules/a.js",
            ".git/objects/ab/cdef",
            "src/nested/generated/out.rs",
        ] {
            assert!(!relevance.admits(dropped), "{dropped} should be dropped");
        }
        for kept in [
            "target",
            "src/main.rs",
            "src/nested/lib.rs",
            "build.log",
            ".git",
            "README.md",
        ] {
            assert!(relevance.admits(kept), "{kept} should be kept");
        }
    }

    #[test]
    fn an_ignore_file_change_updates_the_filter() {
        let root = Root::new();
        root.write(".gitignore", "dist/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert!(!relevance.admits("dist/app.js"));
        assert!(relevance.admits("out/app.js"));

        root.write(".gitignore", "out/\n");
        assert!(relevance.admits(".gitignore"));
        assert!(relevance.admits("dist/app.js"));
        assert!(!relevance.admits("out/app.js"));
    }

    #[test]
    fn a_repository_exclude_change_updates_the_filter() {
        let root = Root::new();
        root.write(".git/info/exclude", "");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert!(relevance.admits("scratch/notes.md"));

        root.write(".git/info/exclude", "scratch/\n");
        assert!(!relevance.admits(".git/info/exclude"));
        assert!(!relevance.admits("scratch/notes.md"));
    }

    #[test]
    fn a_moved_folder_is_decided_again() {
        let root = Root::new();
        root.write("lib/.gitignore", "cache/\n");
        let mut relevance = NoticeRelevance::new(&root.0, None);
        assert!(!relevance.admits("lib/cache/blob"));

        fs::remove_file(root.0.join("lib/.gitignore")).unwrap();
        assert!(relevance.admits("lib"));
        assert!(relevance.admits("lib/cache/blob"));
    }
}
