//! A cheap fingerprint of the Git files that change when a commit, checkout,
//! stage, merge, fetch or push happens outside Sage (EP-12). The Workbench
//! asks for it every second while it is visible, so it only reads metadata of
//! a few files per root and never runs Git.

use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    io::Read,
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};

use tauri::State;

use crate::state::AppState;

const MAX_POINTER_BYTES: u64 = 1024;
const MAX_REMOTES: usize = 8;
/// Files in the Git directory of the checkout (a linked worktree has its own).
const CHECKOUT_FILES: &[&str] = &[
    "HEAD",
    "index",
    "FETCH_HEAD",
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "rebase-merge",
    "rebase-apply",
];

#[tauri::command]
pub async fn get_repository_change_stamp(state: State<'_, AppState>) -> Result<String, String> {
    let project_id = state
        .database
        .active_project_id()
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "no project is active".to_owned())?;
    let roots = state
        .database
        .root_records(&project_id)
        .map_err(|error| error.to_string())?
        .into_iter()
        .map(|root| (root.id, root.absolute_path))
        .collect::<Vec<_>>();
    tauri::async_runtime::spawn_blocking(move || {
        let mut hasher = DefaultHasher::new();
        for (root_id, path) in roots {
            root_id.hash(&mut hasher);
            change_stamp(&path).hash(&mut hasher);
        }
        format!("{:016x}", hasher.finish())
    })
    .await
    .map_err(|error| error.to_string())
}

/// Changes whenever `HEAD`, the index, the current branch, its remote
/// branches, packed refs or an in-progress merge or rebase change.
pub(crate) fn change_stamp(root: &Path) -> u64 {
    let mut hasher = DefaultHasher::new();
    let dot_git = root.join(".git");
    // Only whether it is there: Git creates and renames lock files in it.
    fs::symlink_metadata(&dot_git)
        .ok()
        .map(|metadata| metadata.is_dir())
        .hash(&mut hasher);
    let Some(git_dir) = git_dir(root, &dot_git) else {
        return hasher.finish();
    };
    let common = common_dir(&git_dir);
    for name in CHECKOUT_FILES {
        hash_metadata(&mut hasher, &git_dir.join(name));
    }
    for name in ["packed-refs", "FETCH_HEAD"] {
        hash_metadata(&mut hasher, &common.join(name));
    }
    if let Some(branch) = read_pointer(&git_dir.join("HEAD"))
        .as_deref()
        .and_then(|head| head.strip_prefix("ref: refs/heads/"))
        .filter(|branch| safe_ref_path(branch))
    {
        hash_metadata(&mut hasher, &common.join("refs/heads").join(branch));
        if let Ok(remotes) = fs::read_dir(common.join("refs/remotes")) {
            let mut remotes = remotes
                .flatten()
                .take(MAX_REMOTES)
                .map(|entry| entry.path())
                .collect::<Vec<_>>();
            remotes.sort();
            for remote in remotes {
                hash_metadata(&mut hasher, &remote.join(branch));
            }
        }
    }
    hasher.finish()
}

fn hash_metadata(hasher: &mut DefaultHasher, path: &Path) {
    let Ok(metadata) = fs::symlink_metadata(path) else {
        0_u8.hash(hasher);
        return;
    };
    1_u8.hash(hasher);
    metadata.is_dir().hash(hasher);
    metadata.len().hash(hasher);
    metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .hash(hasher);
}

/// `<root>/.git`, or the directory a `.git` file points to (a linked worktree
/// or a submodule).
fn git_dir(root: &Path, dot_git: &Path) -> Option<PathBuf> {
    let metadata = fs::symlink_metadata(dot_git).ok()?;
    if metadata.is_dir() {
        return Some(dot_git.to_path_buf());
    }
    if !metadata.is_file() {
        return None;
    }
    let pointer = read_pointer(dot_git)?;
    let target = Path::new(pointer.strip_prefix("gitdir:")?.trim());
    Some(root.join(target))
}

fn common_dir(git_dir: &Path) -> PathBuf {
    read_pointer(&git_dir.join("commondir"))
        .map(|common| git_dir.join(common))
        .unwrap_or_else(|| git_dir.to_path_buf())
}

/// The first line of a small text file.
fn read_pointer(path: &Path) -> Option<String> {
    let mut text = String::new();
    fs::File::open(path)
        .ok()?
        .take(MAX_POINTER_BYTES)
        .read_to_string(&mut text)
        .ok()?;
    let line = text.lines().next()?.trim();
    (!line.is_empty()).then(|| line.to_owned())
}

fn safe_ref_path(branch: &str) -> bool {
    !branch.is_empty()
        && branch
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
        && !branch.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().expect("parent")).expect("dir");
        fs::write(path, text).expect("write");
    }

    struct TempTree(PathBuf);

    impl TempTree {
        fn new() -> Self {
            let path =
                std::env::temp_dir().join(format!("sage-change-stamp-{}", uuid::Uuid::new_v4()));
            fs::create_dir(&path).expect("temp");
            Self(path)
        }

        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempTree {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn repository() -> (TempTree, PathBuf) {
        let temp = TempTree::new();
        let git = temp.path().join(".git");
        write(&git.join("HEAD"), "ref: refs/heads/main\n");
        write(&git.join("refs/heads/main"), "a\n");
        fs::create_dir_all(git.join("objects")).expect("objects");
        fs::create_dir_all(git.join("logs")).expect("logs");
        (temp, git)
    }

    #[test]
    fn stamp_is_stable_while_nothing_changes() {
        let (temp, _) = repository();
        assert_eq!(change_stamp(temp.path()), change_stamp(temp.path()));
    }

    #[test]
    fn stamp_changes_with_head_index_branch_and_remote_branch() {
        let (temp, git) = repository();
        let mut last = change_stamp(temp.path());
        for (path, text) in [
            ("index", "staged"),
            ("refs/heads/main", "commit b\n"),
            ("refs/remotes/origin/main", "fetched\n"),
            ("packed-refs", "# packed\n"),
            ("MERGE_HEAD", "c\n"),
            ("HEAD", "ref: refs/heads/topic\n"),
        ] {
            write(&git.join(path), text);
            let next = change_stamp(temp.path());
            assert_ne!(next, last, "{path} did not change the stamp");
            last = next;
        }
    }

    #[test]
    fn stamp_ignores_objects_and_logs() {
        let (temp, git) = repository();
        let before = change_stamp(temp.path());
        write(&git.join("objects/ab/cdef"), "blob");
        write(&git.join("logs/HEAD"), "log");
        write(&git.join("index.lock"), "lock");
        assert_eq!(change_stamp(temp.path()), before);
    }

    #[test]
    fn stamp_notices_a_repository_appearing() {
        let temp = TempTree::new();
        let before = change_stamp(temp.path());
        write(&temp.path().join(".git/HEAD"), "ref: refs/heads/main\n");
        assert_ne!(change_stamp(temp.path()), before);
    }

    #[test]
    fn linked_worktree_reads_its_own_head_and_the_shared_branches() {
        let (main, git) = repository();
        let worktree_git = git.join("worktrees/feature");
        write(&worktree_git.join("HEAD"), "ref: refs/heads/feature\n");
        write(&worktree_git.join("commondir"), "../..\n");
        let worktree = TempTree::new();
        write(
            &worktree.path().join(".git"),
            &format!("gitdir: {}\n", worktree_git.display()),
        );
        let before = change_stamp(worktree.path());
        write(&git.join("refs/heads/feature"), "commit\n");
        let after_branch = change_stamp(worktree.path());
        assert_ne!(after_branch, before);
        write(&worktree_git.join("index"), "staged");
        assert_ne!(change_stamp(worktree.path()), after_branch);
        drop(main);
    }

    #[test]
    fn ref_paths_cannot_leave_the_refs_directory() {
        assert!(safe_ref_path("feature/one"));
        assert!(!safe_ref_path("../../outside"));
        assert!(!safe_ref_path("a//b"));
        assert!(!safe_ref_path("a\\b"));
        assert!(!safe_ref_path(""));
    }
}
