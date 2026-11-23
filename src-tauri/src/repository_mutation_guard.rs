//! A short lease that keeps agent turns out of a repository while Sage changes
//! its working tree (a checkout today; a pull, rebase or stash can reuse it).
//!
//! Repository identity is resolved with no lock held. The store writer is then
//! held only to check for active turns and record the lease, both in one
//! transaction. The Git work runs after that lock is released. Turn openings
//! (`PrepareTurn`, `OpenHarnessTurn`) check the lease inside their own
//! transaction on the same writer, so neither side can slip between the
//! other's check and commit. The lease lives in a TEMP table on the writer
//! connection: SQLite may page it to a temporary file, but it is never part of
//! the profile database and cannot outlive the process. `Drop` clears it on
//! success, error and unwind alike, except when Git may still be running.

use std::future::Future;

use rusqlite::{OptionalExtension, params};
use sage_core::error::{AppError, AppResult, GitContainmentState};
use sage_db::{database::Database, error::SqliteResultExt};
use sage_workspace::Workspace;

use crate::project_registry::{MAX_PROJECTS, MAX_ROOTS_PER_PROJECT};

mod identity;

/// What Sage does to the repository, as the verb phrase its refusals use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct RepositoryOperation(&'static str);

impl RepositoryOperation {
    pub(crate) const SWITCH_BRANCHES: Self = Self("switch branches");

    pub(crate) fn busy(self) -> String {
        format!(
            "Can't {} while an agent is working in this repository.",
            self.0
        )
    }

    fn refused(self, reason: &str) -> String {
        format!("Can't {}: {reason}", self.0)
    }
}

/// A turn start refused by any repository lease, whatever the operation.
pub(crate) const TURN_BLOCKED: &str =
    "Can't start a turn while Sage is changing files in this repository.";
/// Appended to a Git containment error when the lease outlives the call.
pub(crate) const LEASE_KEPT: &str =
    "Agent turns in this repository stay paused until Sage restarts.";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RepositoryPause {
    ChangingFiles,
    UntilRestart,
}

impl RepositoryPause {
    pub(crate) fn message(self) -> &'static str {
        match self {
            Self::ChangingFiles => TURN_BLOCKED,
            Self::UntilRestart => {
                "Can't start a turn while Sage is changing files in this repository. Agent turns in this repository stay paused until Sage restarts."
            }
        }
    }
}

impl std::fmt::Display for RepositoryPause {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message())
    }
}

// The registry admits at most this many roots; more means the store is
// corrupt, and the guard fails closed rather than probe an unbounded set.
const MAX_REGISTERED_ROOTS: i64 = MAX_PROJECTS * MAX_ROOTS_PER_PROJECT;
const MAX_PROJECT_NAME_CHARS: usize = 80;
const ACTIVE_TURN_STATES: &str = "'queued','sending','running','waitingModel','waitingTool',\
     'waitingApproval','waitingUser','cancelling'";

/// Runs `op` while no agent turn can run or start in a Project that reaches
/// `workspace`'s repository. Refuses up front if one already is.
///
/// When `op` reports Git as possibly still running, the lease is kept until
/// Sage restarts: sage-git offers no exit notification to release it on, and
/// a turn must not start while Git may still be rewriting the tree.
pub(crate) async fn with_idle_repository<T, F, Fut>(
    database: &Database,
    workspace: &Workspace,
    operation: RepositoryOperation,
    op: F,
) -> AppResult<T>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = AppResult<T>>,
{
    let mut lease = acquire(database, workspace, operation).await?;
    let result = op().await;
    if let Err(AppError::GitContainment {
        state: GitContainmentState::PotentiallyRunning,
        ..
    }) = &result
    {
        lease.kept = true;
    }
    drop(lease);
    result
}

/// Why a repository lease pauses `project_id`, if any. Turn openings call
/// this inside their transaction on the store writer; a kept lease wins.
pub(crate) fn project_pause(
    connection: &rusqlite::Connection,
    project_id: &str,
) -> rusqlite::Result<Option<RepositoryPause>> {
    let table: Option<i64> = connection
        .query_row(
            "SELECT 1 FROM temp.sqlite_master
             WHERE type='table' AND name='repository_leases'",
            [],
            |row| row.get(0),
        )
        .optional()?;
    if table.is_none() {
        return Ok(None);
    }
    connection.query_row(
        "SELECT MAX(kept) FROM temp.repository_leases WHERE project_id=?1",
        [project_id],
        |row| {
            Ok(row.get::<_, Option<bool>>(0)?.map(|kept| {
                if kept {
                    RepositoryPause::UntilRestart
                } else {
                    RepositoryPause::ChangingFiles
                }
            }))
        },
    )
}

struct Lease {
    database: Database,
    id: String,
    kept: bool,
}

impl Drop for Lease {
    fn drop(&mut self) {
        if self.kept {
            if let Err(error) = self.database.connection().execute(
                "UPDATE temp.repository_leases SET kept=1 WHERE lease_id=?1",
                [&self.id],
            ) {
                tracing::error!(
                    ?error,
                    "repository lease restart hint could not be recorded"
                );
            }
            tracing::warn!("repository lease kept until restart: Git may still be running");
            return;
        }
        let connection = self.database.connection();
        // Retry once so one transient DELETE failure does not keep a
        // completed operation's Project paused until restart.
        for attempt in 0..2 {
            match connection.execute(
                "DELETE FROM temp.repository_leases WHERE lease_id=?1",
                [&self.id],
            ) {
                Ok(_) => break,
                Err(error) => {
                    tracing::error!(?error, attempt, "repository lease could not be cleared")
                }
            }
        }
    }
}

async fn acquire(
    database: &Database,
    workspace: &Workspace,
    operation: RepositoryOperation,
) -> AppResult<Lease> {
    let deadline = tokio::time::Instant::now() + identity::TOTAL_PROBE_TIMEOUT;
    let target = identity::identify_bounded(workspace.root(), deadline)
        .await
        .ok_or_else(|| {
            AppError::InvalidRequest(
                operation.refused("Sage couldn't reach the repository folder."),
            )
        })?;
    let snapshot = registered_roots(&database.connection(), operation)?;
    let classification = identity::classify(&target, &snapshot, deadline).await;
    let id = uuid::Uuid::new_v4().to_string();
    let mut connection = database.connection();
    let transaction = connection.transaction().db()?;
    // A Project is left out of the lease only if every folder it has now was
    // verified as unrelated; one added or changed meanwhile is leased.
    let mut current = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (project, folder) in registered_roots(&transaction, operation)? {
        current.entry(project).or_default().push(folder);
    }
    let leased: Vec<&String> = current
        .iter_mut()
        .filter_map(|(project, folders)| {
            folders.sort();
            (classification.unrelated.get(project) != Some(&*folders)).then_some(project)
        })
        .collect();
    let mut busy = transaction
        .prepare(&format!(
            "SELECT DISTINCT t.project_id,p.name FROM turns t JOIN projects p ON p.id=t.project_id
             WHERE t.state IN ({ACTIVE_TURN_STATES})"
        ))
        .db()?;
    let mut rows = busy.query([]).db()?;
    let mut unverified_busy = None;
    while let Some(row) = rows.next().db()? {
        let project: String = row.get(0).db()?;
        if !leased.contains(&&project) {
            continue;
        }
        if !classification.unverified.contains(&project) {
            return Err(AppError::InvalidRequest(operation.busy()));
        }
        unverified_busy.get_or_insert(row.get::<_, String>(1).db()?);
    }
    drop(rows);
    drop(busy);
    if let Some(name) = unverified_busy {
        let name: String = name.chars().take(MAX_PROJECT_NAME_CHARS).collect();
        return Err(AppError::InvalidRequest(operation.refused(&format!(
            "Sage couldn't verify the folder of project \"{name}\", where an agent is \
             working. Reconnect or remove that folder, then retry."
        ))));
    }
    transaction
        .execute_batch(
            "CREATE TEMP TABLE IF NOT EXISTS repository_leases(
               lease_id TEXT NOT NULL,
               project_id TEXT NOT NULL,
               kept INTEGER NOT NULL DEFAULT 0,
               PRIMARY KEY (lease_id, project_id))",
        )
        .db()?;
    for project in leased {
        transaction
            .execute(
                "INSERT INTO temp.repository_leases(lease_id,project_id) VALUES(?1,?2)",
                params![id, project],
            )
            .db()?;
    }
    transaction.commit().db()?;
    Ok(Lease {
        database: database.clone(),
        id,
        kept: false,
    })
}

fn registered_roots(
    connection: &rusqlite::Connection,
    operation: RepositoryOperation,
) -> AppResult<Vec<(String, String)>> {
    let mut statement = connection
        .prepare("SELECT project_id,absolute_path FROM project_roots LIMIT ?1")
        .db()?;
    let roots = statement
        .query_map([MAX_REGISTERED_ROOTS + 1], |row| {
            Ok((row.get(0)?, row.get(1)?))
        })
        .db()?
        .collect::<Result<Vec<(String, String)>, _>>()
        .db()?;
    if roots.len() as i64 > MAX_REGISTERED_ROOTS {
        return Err(AppError::ResourceLimit(
            operation.refused("the Project registry exceeds its safety limit."),
        ));
    }
    Ok(roots)
}
