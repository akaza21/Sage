//! Recognition uses filesystem metadata only. This module does not invoke Git
//! CLI, follow directory reparse/symlinks, or scan curated vendor/VCS trees.

use std::{
    collections::VecDeque,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use sage_domain::repository::RepositoryRef;

pub(crate) const MAX_REPOSITORIES_PER_ROOT: usize = 8;
pub const MAX_REPOSITORIES_PER_PROJECT: usize = 32;
pub(crate) const MAX_DISCOVERY_DEPTH: usize = 6;
pub(crate) const MAX_VISITED_DIRECTORIES_PER_ROOT: usize = 4_096;
pub(crate) const MAX_ENTRIES_PER_ROOT: usize = 16_384;
pub(crate) const MAX_DISCOVERY_DURATION_PER_ROOT: Duration = Duration::from_millis(750);

const SKIP_DIRECTORY_NAMES: &[&str] = &[
    ".git",
    ".hg",
    ".svn",
    ".bzr",
    "node_modules",
    "target",
    "dist",
    "build",
    "vendor",
    "__pycache__",
    ".venv",
    "venv",
    ".tox",
    ".mypy_cache",
    ".pytest_cache",
    ".next",
    ".nuxt",
    ".svelte-kit",
    ".cache",
    "Pods",
    "Carthage",
];

/// A root outside any Git work tree lists the repositories directly inside it
/// (a folder of checkouts). These caps bound that one-level walk.
pub const MAX_CHILD_REPOSITORIES_PER_ROOT: usize = 32;
pub const MAX_CHILD_ENTRIES_PER_ROOT: usize = 1_024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RootDiscovery {
    pub refs: Vec<RepositoryRef>,
    pub truncated: bool,
    /// The root is outside any Git work tree and `refs` are its direct child
    /// repositories, sorted by path.
    pub child_repositories: bool,
}

struct WalkLimits {
    repositories: usize,
    depth: usize,
    entries: usize,
}

pub fn discover_root(root_id: &str, root: &Path, project_remaining: usize) -> RootDiscovery {
    if is_git_repository(root) || inside_work_tree(root) {
        let (refs, truncated) = walk(
            root_id,
            root,
            WalkLimits {
                repositories: MAX_REPOSITORIES_PER_ROOT.min(project_remaining),
                depth: MAX_DISCOVERY_DEPTH,
                entries: MAX_ENTRIES_PER_ROOT,
            },
        );
        return RootDiscovery {
            refs,
            truncated,
            child_repositories: false,
        };
    }
    let (mut refs, truncated) = walk(
        root_id,
        root,
        WalkLimits {
            repositories: MAX_CHILD_REPOSITORIES_PER_ROOT.min(project_remaining),
            depth: 1,
            entries: MAX_CHILD_ENTRIES_PER_ROOT,
        },
    );
    refs.sort_by(|left, right| left.relative_path.cmp(&right.relative_path));
    RootDiscovery {
        refs,
        truncated,
        child_repositories: true,
    }
}

/// Filesystem-only: some ancestor directory holds a `.git` entry.
fn inside_work_tree(root: &Path) -> bool {
    root.ancestors().skip(1).any(is_git_repository)
}

fn walk(root_id: &str, root: &Path, limits: WalkLimits) -> (Vec<RepositoryRef>, bool) {
    let started = Instant::now();
    let cap = limits.repositories;
    let mut refs = Vec::new();
    let mut truncated = false;
    let mut visited = 0_usize;
    let mut entries_seen = 0_usize;
    let mut queue = VecDeque::from([(root.to_path_buf(), String::new(), 0_usize)]);

    'scan: while let Some((directory, relative, depth)) = queue.pop_front() {
        if started.elapsed() >= MAX_DISCOVERY_DURATION_PER_ROOT
            || visited >= MAX_VISITED_DIRECTORIES_PER_ROOT
            || entries_seen >= limits.entries
        {
            truncated = true;
            break;
        }
        visited += 1;

        if is_git_repository(&directory) && refs.len() < cap {
            refs.push(RepositoryRef {
                root_id: root_id.into(),
                relative_path: relative.clone(),
            });
        } else if is_git_repository(&directory) {
            truncated = true;
            break;
        }

        if depth >= limits.depth {
            continue;
        }
        let Ok(entries) = fs::read_dir(&directory) else {
            continue;
        };
        for entry in entries.flatten() {
            entries_seen += 1;
            if entries_seen >= limits.entries {
                truncated = true;
                break 'scan;
            }
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            if SKIP_DIRECTORY_NAMES.contains(&name) {
                continue;
            }
            let child = directory.join(name);
            if is_symlink_or_reparse(&child) {
                continue;
            }
            let Ok(metadata) = fs::symlink_metadata(&child) else {
                continue;
            };
            if !metadata.is_dir() {
                continue;
            }
            let child_relative = if relative.is_empty() {
                name.to_string()
            } else {
                format!("{relative}/{name}")
            };
            queue.push_back((child, child_relative, depth + 1));
        }
        if refs.len() >= cap {
            if !queue.is_empty() {
                truncated = true;
            }
            break;
        }
    }

    (refs, truncated)
}

fn is_git_repository(directory: &Path) -> bool {
    let git = directory.join(".git");
    let Ok(metadata) = fs::symlink_metadata(&git) else {
        return false;
    };
    if is_symlink_or_reparse(&git) {
        return false;
    }
    metadata.is_dir() || metadata.is_file()
}

fn is_symlink_or_reparse(path: &Path) -> bool {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        return false;
    };
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn scratch(name: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "sage-discovery-{name}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos()
        ));
        fs::create_dir_all(&path).expect("scratch");
        path
    }

    #[test]
    fn discover_root_finds_project_and_nested_git_directories() {
        let root = scratch("nested");
        fs::create_dir_all(root.join(".git")).unwrap();
        fs::create_dir_all(root.join("packages/tool/.git")).unwrap();
        fs::create_dir_all(root.join("node_modules/dep/.git")).unwrap();

        let discovery = discover_root("root-a", &root, MAX_REPOSITORIES_PER_PROJECT);
        fs::remove_dir_all(&root).ok();

        assert_eq!(
            discovery.refs,
            vec![
                RepositoryRef {
                    root_id: "root-a".into(),
                    relative_path: String::new(),
                },
                RepositoryRef {
                    root_id: "root-a".into(),
                    relative_path: "packages/tool".into(),
                },
            ]
        );
        assert!(!discovery.truncated);
        assert!(!discovery.child_repositories);
    }

    #[test]
    fn folder_of_repositories_lists_only_direct_child_repositories() {
        let root = scratch("children");
        fs::create_dir_all(root.join("web/.git")).unwrap();
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(root.join("api/.git"), "gitdir: ../elsewhere\n").unwrap();
        fs::create_dir_all(root.join("notes")).unwrap();
        fs::create_dir_all(root.join("group/deep/.git")).unwrap();
        fs::create_dir_all(root.join("node_modules/.git")).unwrap();

        let discovery = discover_root("root-a", &root, MAX_REPOSITORIES_PER_PROJECT);
        fs::remove_dir_all(&root).ok();

        let paths = discovery
            .refs
            .iter()
            .map(|r| r.relative_path.as_str())
            .collect::<Vec<_>>();
        assert_eq!(paths, ["api", "web"]);
        assert!(discovery.child_repositories);
        assert!(!discovery.truncated);
    }

    #[test]
    fn folder_of_repositories_fails_closed_past_the_repository_cap() {
        let root = scratch("child-cap");
        for index in 0..=MAX_CHILD_REPOSITORIES_PER_ROOT {
            fs::create_dir_all(root.join(format!("repo-{index:03}/.git"))).unwrap();
        }
        let discovery = discover_root("root-a", &root, MAX_REPOSITORIES_PER_PROJECT);
        let limited = discover_root("root-a", &root, 3);
        fs::remove_dir_all(&root).ok();

        assert_eq!(discovery.refs.len(), MAX_CHILD_REPOSITORIES_PER_ROOT);
        assert!(discovery.truncated);
        assert_eq!(limited.refs.len(), 3);
        assert!(limited.truncated);
    }

    #[test]
    fn folder_of_repositories_fails_closed_past_the_entry_cap() {
        let root = scratch("child-entries");
        for index in 0..MAX_CHILD_ENTRIES_PER_ROOT {
            fs::write(root.join(format!("file-{index:05}")), "").unwrap();
        }
        fs::create_dir_all(root.join("zz-repo/.git")).unwrap();
        let discovery = discover_root("root-a", &root, MAX_REPOSITORIES_PER_PROJECT);
        fs::remove_dir_all(&root).ok();

        assert!(discovery.truncated);
        assert!(discovery.refs.len() <= 1);
    }

    #[test]
    fn subfolder_of_a_repository_keeps_the_nested_walk() {
        let root = scratch("inside");
        fs::create_dir_all(root.join(".git")).unwrap();
        let sub = root.join("src");
        fs::create_dir_all(sub.join("a/b/.git")).unwrap();
        let discovery = discover_root("root-a", &sub, MAX_REPOSITORIES_PER_PROJECT);
        fs::remove_dir_all(&root).ok();

        assert!(!discovery.child_repositories);
        assert_eq!(discovery.refs.len(), 1);
        assert_eq!(discovery.refs[0].relative_path, "a/b");
    }
}
