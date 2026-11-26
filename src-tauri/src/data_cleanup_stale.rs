//! Chats not used since a cutoff, across every Project. Counting and the
//! Project list read the same `CleanupScope::Stale` condition the purge
//! admits on, so the confirm count and the delete agree.

use sage_core::error::{AppError, AppResult};
use sage_db::{database::Database, error::SqliteResultExt};

use crate::data_cleanup::{CleanupScope, scope_condition};

const DAY_MILLIS: i64 = 24 * 60 * 60 * 1000;
/// Projects one stale cleanup visits; the rest wait for the next run.
pub(crate) const MAX_STALE_PROJECTS: i64 = 1024;

/// A cutoff at least a day in the past, so nothing in use today can go.
pub(crate) fn stale_scope(cutoff: i64, now: i64) -> AppResult<CleanupScope> {
    if cutoff <= 0 || cutoff > now.saturating_sub(DAY_MILLIS) {
        return Err(AppError::InvalidRequest(
            "Choose a cutoff at least a day ago".into(),
        ));
    }
    Ok(CleanupScope::Stale(cutoff))
}

pub(crate) trait StaleChatStore {
    /// Chats a stale cleanup at `scope` would remove, in every Project.
    fn stale_chat_count(&self, scope: CleanupScope) -> AppResult<u64>;
    /// Projects holding such Chats, at most `MAX_STALE_PROJECTS`.
    fn stale_projects(&self, scope: CleanupScope) -> AppResult<Vec<String>>;
}

impl StaleChatStore for Database {
    fn stale_chat_count(&self, scope: CleanupScope) -> AppResult<u64> {
        let condition = scope_condition(scope);
        self.connection()
            .query_row(
                &format!("SELECT COUNT(*) FROM chats c WHERE {condition}"),
                [],
                |row| row.get::<_, i64>(0),
            )
            .db()
            .map(|count| count.max(0) as u64)
    }

    fn stale_projects(&self, scope: CleanupScope) -> AppResult<Vec<String>> {
        let condition = scope_condition(scope);
        let connection = self.connection();
        let mut statement = connection
            .prepare(&format!(
                "SELECT DISTINCT c.project_id FROM chats c WHERE {condition}
                 ORDER BY c.project_id LIMIT ?1"
            ))
            .db()?;
        statement
            .query_map([MAX_STALE_PROJECTS], |row| row.get(0))
            .db()?
            .collect::<Result<_, _>>()
            .db()
    }
}
