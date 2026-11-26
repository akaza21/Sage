//! Bulk removal of old data: every archived Chat of a Project, or a whole
//! Project together with its Chats. Each Chat goes through the single-Chat
//! purge, forks before the Chats they were forked from, so lineage, images
//! and usage are released exactly as a one-by-one delete releases them.

use rusqlite::{Connection, OptionalExtension, params};
use sage_core::error::{AppError, AppResult};
use sage_db::{database::Database, error::SqliteResultExt};

use crate::{
    acp::service::ChatRuntimeRelease, chat_lifecycle::validate_chat_identifier,
    chat_purge::ChatPurgeStore,
};

/// Chats read per page; the next page is read after the page is purged.
const PAGE: i64 = 256;
/// Chats whose runtime the command releases before purging; a Chat past
/// this that still holds a runtime is refused by the purge's admission.
pub(crate) const MAX_RELEASED_CHATS: i64 = 4096;

/// Which Chats of a Project a cleanup removes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CleanupScope {
    /// Chats attached to no tab.
    Archived,
    /// Every Chat, and then the Project itself.
    Project,
}

const ARCHIVED: &str = "NOT EXISTS(SELECT 1 FROM chat_tabs t WHERE t.chat_id=c.id)
    AND NOT EXISTS(SELECT 1 FROM workbench_tabs w WHERE w.project_id=c.project_id AND w.id=c.id)";

pub(crate) trait DataCleanupStore {
    /// Refuses before any runtime is released: a missing or internal
    /// Project, a pending Trash recovery, or a running Turn.
    fn admit_cleanup(&self, project: &str, scope: CleanupScope) -> AppResult<()>;
    /// Whether `chat` is still one `scope` removes; runtime release asks.
    fn still_in_scope(&self, project: &str, chat: &str, scope: CleanupScope) -> bool;
    /// The Chats `scope` would remove, at most `limit`, for runtime release.
    fn cleanup_chat_ids(
        &self,
        project: &str,
        scope: CleanupScope,
        limit: i64,
    ) -> AppResult<Vec<String>>;
    /// Purges the Chats of `scope` and, for `CleanupScope::Project`, the
    /// Project. Each Chat commits on its own, so a refusal after the first
    /// one is returned beside the count instead of as an error.
    fn clean_up(&self, project: &str, scope: CleanupScope) -> AppResult<CleanupOutcome>;
}

/// What a cleanup removed, and why it stopped early if it did.
#[derive(Debug)]
pub(crate) struct CleanupOutcome {
    pub(crate) deleted: usize,
    pub(crate) refusal: Option<AppError>,
}

impl DataCleanupStore for Database {
    fn admit_cleanup(&self, project: &str, scope: CleanupScope) -> AppResult<()> {
        validate_chat_identifier("Project ID", project)?;
        admit_project(&self.connection(), project, scope, false)
    }

    fn still_in_scope(&self, project: &str, chat: &str, scope: CleanupScope) -> bool {
        let archived = if scope == CleanupScope::Archived {
            ARCHIVED
        } else {
            "1"
        };
        self.connection()
            .query_row(
                &format!(
                    "SELECT EXISTS(SELECT 1 FROM chats c WHERE c.project_id=?1 AND c.id=?2 AND {archived})"
                ),
                params![project, chat],
                |row| row.get(0),
            )
            .unwrap_or(false)
    }

    fn cleanup_chat_ids(
        &self,
        project: &str,
        scope: CleanupScope,
        limit: i64,
    ) -> AppResult<Vec<String>> {
        validate_chat_identifier("Project ID", project)?;
        chat_page(&self.connection(), project, scope, false, limit)
    }

    fn clean_up(&self, project: &str, scope: CleanupScope) -> AppResult<CleanupOutcome> {
        validate_chat_identifier("Project ID", project)?;
        admit_project(&self.connection(), project, scope, true)?;
        let mut deleted = 0;
        let stopped = (|| -> AppResult<()> {
            loop {
                let page = chat_page(&self.connection(), project, scope, true, PAGE)?;
                if page.is_empty() {
                    break;
                }
                for chat in page {
                    self.purge_chat(project, &chat, |connection| {
                        admit_chat(connection, project, &chat, scope)
                    })?;
                    deleted += 1;
                }
            }
            if scope == CleanupScope::Project {
                forget_project(self, project)?;
            } else if !chat_page(&self.connection(), project, scope, false, 1)?.is_empty() {
                // Only Chats with an attached fork remain.
                return Err(AppError::InvalidRequest(
                    "Archived chats with open forks were kept; delete the forks first".into(),
                ));
            }
            Ok(())
        })()
        .err();
        match stopped {
            Some(error) if deleted == 0 => Err(error),
            refusal => Ok(CleanupOutcome { deleted, refusal }),
        }
    }
}

/// Chats of the scope, forks first when `leaves_only`: a Chat other Chats
/// were forked from waits until those forks are gone.
fn chat_page(
    connection: &Connection,
    project: &str,
    scope: CleanupScope,
    leaves_only: bool,
    limit: i64,
) -> AppResult<Vec<String>> {
    let archived = if scope == CleanupScope::Archived {
        ARCHIVED
    } else {
        "1"
    };
    let leaves = if leaves_only {
        "NOT EXISTS(SELECT 1 FROM chat_lineage l
           WHERE l.parent_project_id=c.project_id AND l.parent_chat_id=c.id)"
    } else {
        "1"
    };
    let mut statement = connection
        .prepare(&format!(
            "SELECT c.id FROM chats c WHERE c.project_id=?1 AND {archived} AND {leaves}
             ORDER BY c.id LIMIT ?2"
        ))
        .db()?;
    statement
        .query_map(params![project, limit], |row| row.get(0))
        .db()?
        .collect::<Result<_, _>>()
        .db()
}

/// Refuses before anything is deleted: only a user Project is removed, and
/// nothing in scope may be running a Turn or holding an unsettled runtime.
/// `settled` also refuses live runtimes: false before the command parks
/// idle runtimes, true once they have had the chance to settle.
fn admit_project(
    connection: &Connection,
    project: &str,
    scope: CleanupScope,
    settled: bool,
) -> AppResult<()> {
    let kind: Option<String> = connection
        .query_row("SELECT kind FROM projects WHERE id=?1", [project], |row| {
            row.get(0)
        })
        .optional()
        .db()?;
    match kind.as_deref() {
        None => return Err(AppError::InvalidRequest("Project does not exist".into())),
        Some("user") => {}
        Some(_) if scope == CleanupScope::Project => {
            return Err(AppError::InvalidRequest(
                "Only a user Project can be removed".into(),
            ));
        }
        Some(_) => {}
    }
    let pending_trash: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM resource_trash_intents WHERE project_id=?1)",
            [project],
            |row| row.get(0),
        )
        .db()?;
    if pending_trash && scope == CleanupScope::Project {
        return Err(AppError::InvalidRequest(
            "A move to Trash in this Project is still being recovered; try again".into(),
        ));
    }
    let archived = if scope == CleanupScope::Archived {
        ARCHIVED
    } else {
        "1"
    };
    let (running, unsettled): (bool, bool) = connection
        .query_row(
            &format!(
                "SELECT
                   EXISTS(SELECT 1 FROM turns u JOIN chats c ON c.project_id=u.project_id AND c.id=u.chat_id
                          WHERE u.project_id=?1 AND u.terminal_at IS NULL AND {archived}),
                   EXISTS(SELECT 1 FROM session_runtime_epochs e JOIN chats c ON c.project_id=e.project_id AND c.id=e.chat_id
                          WHERE e.project_id=?1 AND e.state IN ('starting','running','terminating') AND {archived})"
            ),
            [project],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .db()?;
    let refusal = match (scope, running, settled && unsettled) {
        (CleanupScope::Archived, true, _) => ChatRuntimeRelease::TurnRunning.message("deleting"),
        (CleanupScope::Archived, _, true) => ChatRuntimeRelease::Unsettled.message("deleting"),
        (CleanupScope::Project, true, _) => {
            "Stop the running turns in this Project before removing it".into()
        }
        (CleanupScope::Project, _, true) => {
            "This Project's agents are still shutting down; try removing it again".into()
        }
        _ => return Ok(()),
    };
    Err(AppError::InvalidRequest(refusal))
}

/// Runs before the purge and again inside its transaction.
fn admit_chat(
    connection: &Connection,
    project: &str,
    chat: &str,
    scope: CleanupScope,
) -> AppResult<()> {
    let archived = if scope == CleanupScope::Archived {
        ARCHIVED
    } else {
        "1"
    };
    let admitted: bool = connection
        .query_row(
            &format!(
                "SELECT EXISTS(SELECT 1 FROM chats c WHERE c.project_id=?1 AND c.id=?2 AND {archived})
                   AND NOT EXISTS(SELECT 1 FROM turns WHERE project_id=?1 AND chat_id=?2 AND terminal_at IS NULL)
                   AND NOT EXISTS(SELECT 1 FROM session_runtime_epochs WHERE project_id=?1 AND chat_id=?2
                                  AND state IN ('starting','running','terminating'))"
            ),
            params![project, chat],
            |row| row.get(0),
        )
        .db()?;
    if admitted {
        Ok(())
    } else {
        Err(AppError::InvalidRequest(
            "A Chat changed while old data was being removed; try again".into(),
        ))
    }
}

/// Deletes the Project once it has no Chats. Roots, layouts, buffers and
/// tabs go with it through their cascading references.
fn forget_project(database: &Database, project: &str) -> AppResult<()> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction().db()?;
    // Forks share their parent's Project, so every Chat is gone by now;
    // a Chat created meanwhile or a new trash intent keeps the Project.
    let kept: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM chats WHERE project_id=?1)
                 OR EXISTS(SELECT 1 FROM resource_trash_intents WHERE project_id=?1)",
            [project],
            |row| row.get(0),
        )
        .db()?;
    if kept {
        return Err(AppError::InvalidRequest(
            "The Project changed while it was being removed; try again".into(),
        ));
    }
    transaction
        .execute(
            "UPDATE app_state SET active_project_id=NULL WHERE active_project_id=?1",
            [project],
        )
        .db()?;
    transaction
        .execute(
            "DELETE FROM acp_project_usage WHERE project_id=?1",
            [project],
        )
        .db()?;
    transaction
        .execute(
            "DELETE FROM projects WHERE id=?1 AND kind='user'",
            [project],
        )
        .db()?;
    transaction.commit().db()
}

#[cfg(test)]
#[path = "data_cleanup_tests.rs"]
mod tests;
