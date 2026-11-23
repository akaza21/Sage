use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
};

use sage_core::{
    error::{AppError, AppResult},
    win32_path::authority_path_key,
};

const MAX_REPOSITORY_PATH_BYTES: usize = 32 * 1024;
// An offline share or unplugged drive can stall a path probe for tens of
// seconds while the caller holds the Project transition lock. Past this, a
// folder counts as unverified, which the lease already handles safely.
const PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);
// The least a root probe gets, enough for a linked worktree's `git rev-parse`
// to start on Windows; roots left when the total deadline runs out stay
// unverified.
const PROBE_FLOOR: std::time::Duration = std::time::Duration::from_millis(250);
// Shared by the target and every registered root, under project_transitions.
pub(super) const TOTAL_PROBE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Where a folder sits: its canonical authority key and, when it is inside a
/// Git working tree, the canonical key of that repository's common directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FolderIdentity {
    folder: PathBuf,
    repository: Option<OsString>,
}

impl FolderIdentity {
    /// The folders reach the same working files: one is the other or holds
    /// it, or both are working trees of one repository (including linked
    /// worktrees, which share the common directory).
    pub(super) fn overlaps(&self, other: &Self) -> bool {
        self.folder.starts_with(&other.folder)
            || other.folder.starts_with(&self.folder)
            || matches!((&self.repository, &other.repository), (Some(a), Some(b)) if a == b)
    }
}

/// How each Project relates to the target repository, decided without any
/// store lock. Every Project not listed as unrelated is leased. A Project with
/// a folder Sage cannot verify is listed so it can be named if it is busy.
#[derive(Debug, Default)]
pub(super) struct Classification {
    pub unverified: Vec<String>,
    pub unrelated: BTreeMap<String, Vec<String>>,
}

/// identify within PROBE_TIMEOUT; None when the folder can't be verified.
pub(super) async fn identify_bounded(
    root: &Path,
    deadline: tokio::time::Instant,
) -> Option<FolderIdentity> {
    if tokio::time::Instant::now() >= deadline {
        return None;
    }
    tokio::time::timeout_at(
        deadline.min(tokio::time::Instant::now() + PROBE_TIMEOUT),
        identify(root),
    )
    .await
    .ok()?
    .ok()
}

async fn identify(root: &Path) -> AppResult<FolderIdentity> {
    let canonical = tokio::fs::canonicalize(root).await?;
    let folder = PathBuf::from(authority_path_key(&canonical));
    for ancestor in canonical.ancestors() {
        let dot_git = ancestor.join(".git");
        match tokio::fs::symlink_metadata(&dot_git).await {
            // A main working tree's `.git` directory is its common directory.
            Ok(metadata) if metadata.is_dir() => {
                let common = tokio::fs::canonicalize(&dot_git).await?;
                return Ok(FolderIdentity {
                    folder,
                    repository: Some(authority_path_key(&common)),
                });
            }
            // A `.git` file names a linked worktree or submodule: ask Git.
            Ok(_) => {
                return Ok(FolderIdentity {
                    folder,
                    repository: Some(common_directory(ancestor).await?),
                });
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(FolderIdentity {
        folder,
        repository: None,
    })
}

async fn common_directory(working_tree: &Path) -> AppResult<OsString> {
    let output = sage_git::run_git(
        working_tree,
        &["rev-parse", "--path-format=absolute", "--git-common-dir"],
        MAX_REPOSITORY_PATH_BYTES,
    )
    .await?;
    if !output.succeeded() || output.stdout_truncated {
        return Err(AppError::Git("Git could not resolve the repository".into()));
    }
    let path = std::str::from_utf8(&output.stdout)
        .map_err(|_| AppError::Git("Git repository path is not valid UTF-8".into()))?
        .trim_end_matches(['\r', '\n']);
    if path.is_empty() || !Path::new(path).is_absolute() {
        return Err(AppError::Git(
            "Git returned an invalid repository path".into(),
        ));
    }
    Ok(authority_path_key(
        &tokio::fs::canonicalize(Path::new(path)).await?,
    ))
}

/// `roots` holds (project id, folder) rows, already bounded by the caller.
pub(super) async fn classify(
    target: &FolderIdentity,
    roots: &[(String, String)],
    deadline: tokio::time::Instant,
) -> Classification {
    classify_with(target, roots, deadline, |folder| {
        let root = PathBuf::from(folder);
        async move { identify_bounded(&root, deadline).await }
    })
    .await
}

async fn classify_with<F, Fut>(
    target: &FolderIdentity,
    roots: &[(String, String)],
    deadline: tokio::time::Instant,
    mut probe: F,
) -> Classification
where
    F: FnMut(&str) -> Fut,
    Fut: std::future::Future<Output = Option<FolderIdentity>>,
{
    let mut seen = BTreeMap::<&str, Option<bool>>::new();
    let mut projects = BTreeMap::<&str, (Option<bool>, Vec<String>)>::new();
    let mut unprobed = roots
        .iter()
        .map(|(_, folder)| folder.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .len() as u32;
    for (project, folder) in roots {
        let shares = match seen.get(folder.as_str()) {
            Some(known) => *known,
            None => {
                let now = tokio::time::Instant::now();
                // Each folder gets its share of the time left, but at least
                // PROBE_FLOOR, so one stalled probe can't leave the folders
                // after it unprobed and many roots can't starve a slow one.
                let known = if now >= deadline {
                    None
                } else {
                    let left = deadline - now;
                    let budget = (left / unprobed)
                        .max(PROBE_FLOOR)
                        .min(left)
                        .min(PROBE_TIMEOUT);
                    tokio::time::timeout_at(now + budget, probe(folder))
                        .await
                        .ok()
                        .flatten()
                        .map(|identity| identity.overlaps(target))
                };
                unprobed -= 1;
                seen.insert(folder, known);
                known
            }
        };
        let entry = projects.entry(project).or_insert((Some(false), Vec::new()));
        entry.0 = match (entry.0, shares) {
            (Some(true), _) | (_, Some(true)) => Some(true),
            (None, _) | (_, None) => None,
            _ => Some(false),
        };
        entry.1.push(folder.clone());
    }
    let mut classification = Classification::default();
    for (project, (shares, mut folders)) in projects {
        match shares {
            // Sharing Projects are always leased; only the others are listed.
            Some(true) => {}
            None => classification.unverified.push(project.to_owned()),
            Some(false) => {
                folders.sort();
                classification.unrelated.insert(project.to_owned(), folders);
            }
        }
    }
    classification
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn each_root_probe_gets_its_share_of_the_deadline_and_at_least_the_floor() {
        let root = sage_test_support::test_paths::temp_dir()
            .join(format!("sage-probe-deadline-{}", uuid::Uuid::new_v4()));
        let target_root = root.join("target");
        let unrelated_root = root.join("unrelated");
        tokio::fs::create_dir_all(&target_root).await.unwrap();
        tokio::fs::create_dir_all(&unrelated_root).await.unwrap();
        let target = identify(&target_root).await.unwrap();
        let unrelated = unrelated_root.to_str().unwrap().to_owned();
        let roots = vec![
            ("verified".into(), unrelated.clone()),
            ("stalled".into(), "stalled-folder".into()),
            ("unprobed".into(), target_root.to_str().unwrap().to_owned()),
        ];
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let result = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            classify_with(&target, &roots, deadline, |folder| {
                let root = PathBuf::from(folder);
                async move {
                    if root == Path::new("stalled-folder") {
                        std::future::pending().await
                    } else {
                        identify(&root).await.ok()
                    }
                }
            }),
        )
        .await
        .expect("the whole pass is bounded even when a probe never returns");
        assert_eq!(
            result.unrelated,
            BTreeMap::from([("verified".into(), vec![unrelated.clone()])])
        );
        // The stalled folder used only its share: the target's own folder,
        // probed after it, is still verified as shared and leased.
        assert_eq!(result.unverified, ["stalled"]);

        // With many roots, the first still gets the floor, not a sliver.
        let mut roots = vec![("slow".to_owned(), unrelated.clone())];
        roots.extend((0..511).map(|index| (format!("fast-{index:03}"), format!("fast-{index}"))));
        let unrelated_identity = identify(&unrelated_root).await.unwrap();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(1);
        let result = classify_with(&target, &roots, deadline, |folder| {
            let slow = folder == unrelated;
            let identity = unrelated_identity.clone();
            async move {
                if slow {
                    tokio::time::sleep(PROBE_FLOOR / 2).await;
                }
                Some(identity)
            }
        })
        .await;
        assert!(result.unverified.is_empty());
        assert_eq!(result.unrelated.len(), roots.len());
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
