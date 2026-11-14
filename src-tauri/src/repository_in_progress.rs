//! Which Git operation is stopped part way, read with one Git call.
//!
//! Matches `repository_conflict::{merge,rebase,cherry_pick,revert}_in_progress`,
//! which ask Git for each marker path separately; the status refresh runs
//! this instead so it spawns one process, not five or more.

use std::path::Path;

use sage_core::error::{AppError, AppResult};

use crate::repository;
use sage_workspace::Workspace;

const MAX_GIT_ADMIN_PATHS_BYTES: usize = MARKERS.len() * 32 * 1024;
const MARKERS: [&str; 6] = [
    "MERGE_HEAD",
    "CHERRY_PICK_HEAD",
    "REVERT_HEAD",
    "REBASE_HEAD",
    "rebase-merge",
    "rebase-apply",
];

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct InProgress {
    pub merge: bool,
    pub rebase: bool,
    pub cherry_pick: bool,
    pub revert: bool,
}

pub(crate) async fn in_progress(workspace: &Workspace) -> AppResult<InProgress> {
    let mut args = vec!["rev-parse", "--path-format=absolute"];
    for marker in MARKERS {
        args.extend(["--git-path", marker]);
    }
    let output =
        crate::repository_workspace::run_git(workspace, &args, MAX_GIT_ADMIN_PATHS_BYTES + 2)
            .await?;
    repository::ensure_success(&output)?;
    if output.stdout_truncated {
        return Err(AppError::ResourceLimit(
            "Git administrative paths exceed their bounded contract".into(),
        ));
    }
    let text = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git administrative path is not UTF-8".into()))?;
    let paths = text.lines().collect::<Vec<_>>();
    if paths.len() != MARKERS.len() {
        return Err(AppError::Git(
            "Git returned the wrong number of administrative paths".into(),
        ));
    }
    let mut present = [false; MARKERS.len()];
    for (slot, path) in present.iter_mut().zip(paths) {
        *slot = marker_exists(path.trim_end_matches('\r'))?;
    }
    Ok(classify(present))
}

fn marker_exists(path: &str) -> AppResult<bool> {
    if path.is_empty() || path.contains('\0') {
        return Err(AppError::Git(
            "Git returned an invalid administrative path".into(),
        ));
    }
    let path = Path::new(path);
    if !path.is_absolute() {
        return Err(AppError::Git(
            "Git returned a non-absolute administrative path".into(),
        ));
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

/// The same exclusive rules as `repository_conflict`: an operation counts
/// only when no other operation's marker is present too.
fn classify(present: [bool; 6]) -> InProgress {
    let [
        merge_head,
        cherry_pick_head,
        revert_head,
        rebase_head,
        rebase_merge,
        rebase_apply,
    ] = present;
    let rebase_dir = rebase_merge || rebase_apply;
    InProgress {
        merge: merge_head && !cherry_pick_head && !revert_head && !rebase_head && !rebase_dir,
        rebase: rebase_dir && !merge_head && !cherry_pick_head && !revert_head,
        cherry_pick: cherry_pick_head && !merge_head && !revert_head && !rebase_head && !rebase_dir,
        revert: revert_head && !merge_head && !cherry_pick_head && !rebase_head && !rebase_dir,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_matches_the_exclusive_marker_rules() {
        let none = classify([false; 6]);
        assert_eq!(none, InProgress::default());
        let merge = classify([true, false, false, false, false, false]);
        assert!(merge.merge && !merge.rebase && !merge.cherry_pick && !merge.revert);
        let rebase = classify([false, false, false, true, true, false]);
        assert!(rebase.rebase && !rebase.merge && !rebase.cherry_pick);
        let apply = classify([false, false, false, false, false, true]);
        assert!(apply.rebase);
        let cherry = classify([false, true, false, false, false, false]);
        assert!(cherry.cherry_pick && !cherry.merge);
        let revert = classify([false, false, true, false, false, false]);
        assert!(revert.revert && !revert.cherry_pick);
        // A cherry-pick stopped inside a rebase is neither.
        let both = classify([false, true, false, true, true, false]);
        assert!(!both.cherry_pick && !both.rebase);
        let merge_and_revert = classify([true, false, true, false, false, false]);
        assert_eq!(merge_and_revert, InProgress::default());
    }

    /// Every combination of markers gives what the four separate checks give.
    #[test]
    fn one_call_matches_the_separate_checks_for_every_marker_combination() {
        use crate::repository_conflict::{
            cherry_pick_in_progress, merge_in_progress, rebase_in_progress, revert_in_progress,
        };
        let root = std::env::temp_dir().join(format!("sage-in-progress-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let status = crate::git_process::fixture_git()
            .arg("-C")
            .arg(&root)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
        let workspace = Workspace::open("project-a", "root-a", &root).unwrap();
        let runtime = tokio::runtime::Runtime::new().unwrap();
        let git = root.join(".git");
        for mask in 0_u32..64 {
            for (index, marker) in MARKERS.iter().enumerate() {
                let path = git.join(marker);
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::remove_dir_all(&path);
                if mask & (1 << index) != 0 {
                    if marker.starts_with("rebase-") {
                        std::fs::create_dir(&path).unwrap();
                    } else {
                        std::fs::write(&path, "0000000000000000000000000000000000000000\n")
                            .unwrap();
                    }
                }
            }
            let combined = runtime.block_on(in_progress(&workspace)).unwrap();
            let separate = runtime.block_on(async {
                InProgress {
                    merge: merge_in_progress(&workspace).await.unwrap(),
                    rebase: rebase_in_progress(&workspace).await.unwrap(),
                    cherry_pick: cherry_pick_in_progress(&workspace).await.unwrap(),
                    revert: revert_in_progress(&workspace).await.unwrap(),
                }
            });
            assert_eq!(combined, separate, "marker mask {mask:06b}");
        }
        std::fs::remove_dir_all(root).unwrap();
    }
}
