use std::{ffi::OsString, path::Path};

use sage_core::{
    error::{AppError, AppResult},
    win32_path::authority_path_key,
};
use sage_db::error::SqliteResultExt;
use sage_workspace::Workspace;

// A corrupt or unrecovered activity set must fail closed, never start an
// unbounded series of Git probes. No checkout state is retained between calls.
const MAX_ACTIVE_ROOTS: usize = 64;
const MAX_REPOSITORY_PATH_BYTES: usize = 32 * 1024;
const BUSY: &str = "Can't switch branches while an agent is working in this repository.";

pub(super) async fn ensure_repository_idle(
    connection: &rusqlite::Connection,
    workspace: &Workspace,
) -> AppResult<()> {
    let roots = {
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT r.absolute_path,length(CAST(r.absolute_path AS BLOB))
                 FROM project_roots r
                 JOIN turns t ON t.project_id=r.project_id
                 WHERE t.state IN ('queued','sending','running','waitingModel','waitingTool',
                   'waitingApproval','waitingUser','cancelling') LIMIT ?1",
            )
            .db()?;
        let mut rows = statement.query([(MAX_ACTIVE_ROOTS + 1) as i64]).db()?;
        let mut roots = Vec::<String>::new();
        while let Some(row) = rows.next().db()? {
            if row.get::<_, i64>(1).db()? > MAX_REPOSITORY_PATH_BYTES as i64 {
                return Err(AppError::ResourceLimit(
                    "Can't switch branches: an active agent root exceeds its path limit.".into(),
                ));
            }
            roots.push(row.get(0).db()?);
        }
        roots
    };
    if roots.len() > MAX_ACTIVE_ROOTS {
        return Err(AppError::ResourceLimit(
            "Can't switch branches: agent repository activity exceeds its safety limit.".into(),
        ));
    }
    if roots.is_empty() {
        return Ok(());
    }
    let target_root = authority_path_key(&workspace.root().canonicalize()?);
    for root in &roots {
        if authority_path_key(&Path::new(root).canonicalize()?) == target_root {
            return Err(AppError::InvalidRequest(BUSY.into()));
        }
    }
    let target = repository_identity(workspace.root()).await?;
    for root in roots {
        if target.is_some() && repository_identity(Path::new(&root)).await? == target {
            return Err(AppError::InvalidRequest(BUSY.into()));
        }
    }
    Ok(())
}

// Projects may name a subdirectory, an alias, or another linked worktree.
// Resolve the closest repository through Git's existing config/layout admission
// and compare canonical common directories, rather than project IDs or strings.
async fn repository_identity(root: &Path) -> AppResult<Option<OsString>> {
    let root = root.canonicalize()?;
    for ancestor in root.ancestors() {
        match ancestor.join(".git").symlink_metadata() {
            Ok(_) => {
                let output = sage_git::run_git(
                    ancestor,
                    &["rev-parse", "--path-format=absolute", "--git-common-dir"],
                    MAX_REPOSITORY_PATH_BYTES,
                )
                .await?;
                if !output.succeeded() || output.stdout_truncated {
                    return Err(AppError::InvalidRequest(
                        "Can't switch branches: an active agent repository could not be verified."
                            .into(),
                    ));
                }
                let path = std::str::from_utf8(&output.stdout)
                    .map_err(|_| AppError::Git("Git repository path is not valid UTF-8".into()))?
                    .trim_end_matches(['\r', '\n']);
                if path.is_empty() || !Path::new(path).is_absolute() {
                    return Err(AppError::Git(
                        "Git returned an invalid repository path".into(),
                    ));
                }
                return Ok(Some(authority_path_key(&Path::new(path).canonicalize()?)));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(None)
}
