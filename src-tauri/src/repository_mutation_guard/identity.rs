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

pub(super) async fn identify(root: &Path) -> AppResult<FolderIdentity> {
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
) -> Classification {
    let mut seen = BTreeMap::<&str, Option<bool>>::new();
    let mut projects = BTreeMap::<&str, (Option<bool>, Vec<String>)>::new();
    for (project, folder) in roots {
        let shares = match seen.get(folder.as_str()) {
            Some(known) => *known,
            None => {
                let known = identify(Path::new(folder))
                    .await
                    .ok()
                    .map(|identity| identity.overlaps(target));
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
