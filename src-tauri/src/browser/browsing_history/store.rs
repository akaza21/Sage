use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};

pub(super) const MAX_VISITS: i64 = 10_000;
/// Visits older than this are pruned on write, as Chrome keeps 90 days.
pub(super) const RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1000;
pub(super) const MAX_URL_BYTES: usize = super::super::admission::MAX_BROWSER_URL_BYTES;
pub(super) const MAX_TITLE_CHARS: usize = 256;
pub(crate) const MAX_PAGE: u32 = 100;
pub(crate) const MAX_SEARCH_CHARS: usize = 256;
pub(crate) const MAX_REMOVE: usize = 100;
const SCHEMA_VERSION: i64 = 1;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Visit {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub visited_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cursor {
    pub visited_at: i64,
    pub id: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisitPage {
    pub visits: Vec<Visit>,
    pub next: Option<Cursor>,
}

pub(super) fn open(path: &str) -> rusqlite::Result<Connection> {
    let mut connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    // Losing the last few visits on power loss is acceptable; an fsync on
    // every page load is not.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    migrate(&mut connection)?;
    Ok(connection)
}

fn migrate(connection: &mut Connection) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    let version: i64 = transaction.pragma_query_value(None, "user_version", |row| row.get(0))?;
    if version >= SCHEMA_VERSION {
        return Ok(());
    }
    // Version 0 named visits by a plain rowid, which SQLite hands out
    // again once the newest row is removed. Rebuild it: copying the ids
    // into the AUTOINCREMENT table seeds its sequence past all of them.
    let legacy = transaction.query_row(
        "SELECT count(*) FROM sqlite_master WHERE type = 'table' AND name = 'browser_visits'",
        [],
        |row| row.get::<_, i64>(0),
    )? > 0;
    if legacy {
        transaction.execute_batch("ALTER TABLE browser_visits RENAME TO browser_visits_v0;")?;
    }
    // AUTOINCREMENT: the writer names a tab's latest visit by id, so a
    // removed visit's id must never come back as another tab's row.
    transaction.execute_batch(
        "CREATE TABLE browser_visits (
           id INTEGER PRIMARY KEY AUTOINCREMENT,
           url TEXT NOT NULL CHECK (length(url) BETWEEN 1 AND 2048),
           title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 256),
           visited_at INTEGER NOT NULL CHECK (visited_at >= 0)
         ) STRICT;",
    )?;
    if legacy {
        transaction.execute_batch(
            "INSERT INTO browser_visits (id, url, title, visited_at)
               SELECT id, url, title, visited_at FROM browser_visits_v0;
             DROP TABLE browser_visits_v0;",
        )?;
    }
    transaction
        .execute_batch("CREATE INDEX browser_visits_by_time ON browser_visits(visited_at);")?;
    transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
    transaction.commit()
}

pub(super) fn admissible(url: &str, visited_at: i64) -> bool {
    !url.is_empty() && url.len() <= MAX_URL_BYTES && visited_at >= 0
}

fn bounded_title(title: &str) -> Option<String> {
    let title = title.chars().take(MAX_TITLE_CHARS).collect::<String>();
    (!title.is_empty()).then_some(title)
}

pub(super) fn insert(
    transaction: &Transaction<'_>,
    url: &str,
    title: Option<&str>,
    visited_at: i64,
) -> rusqlite::Result<i64> {
    transaction
        .prepare_cached("INSERT INTO browser_visits(url, title, visited_at) VALUES (?1, ?2, ?3)")?
        .execute(params![url, title.and_then(bounded_title), visited_at])?;
    Ok(transaction.last_insert_rowid())
}

pub(super) fn replace_url(
    transaction: &Transaction<'_>,
    id: i64,
    url: &str,
    title: Option<&str>,
) -> rusqlite::Result<bool> {
    let changed = transaction
        .prepare_cached(
            "UPDATE browser_visits SET url = ?2, title = coalesce(?3, title) WHERE id = ?1",
        )?
        .execute(params![id, url, title.and_then(bounded_title)])?;
    Ok(changed == 1)
}

pub(super) fn set_title(
    transaction: &Transaction<'_>,
    id: i64,
    title: &str,
) -> rusqlite::Result<()> {
    transaction
        .prepare_cached("UPDATE browser_visits SET title = ?2 WHERE id = ?1")?
        .execute(params![id, bounded_title(title)])?;
    Ok(())
}

pub(super) fn prune(transaction: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
    transaction
        .prepare_cached("DELETE FROM browser_visits WHERE visited_at < ?1")?
        .execute(params![now.saturating_sub(RETENTION_MS)])?;
    let oldest_kept = transaction
        .prepare_cached(
            "SELECT visited_at, id FROM browser_visits
             ORDER BY visited_at DESC, id DESC LIMIT 1 OFFSET ?1",
        )?
        .query_row(params![MAX_VISITS - 1], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
        })
        .optional()?;
    if let Some((visited_at, id)) = oldest_kept {
        transaction
            .prepare_cached(
                "DELETE FROM browser_visits
                 WHERE visited_at < ?1 OR (visited_at = ?1 AND id < ?2)",
            )?
            .execute(params![visited_at, id])?;
    }
    Ok(())
}

pub(super) fn page(
    connection: &Connection,
    search: &str,
    before: Option<Cursor>,
    limit: u32,
) -> rusqlite::Result<VisitPage> {
    let limit = limit.clamp(1, MAX_PAGE);
    let pattern = format!("%{}%", escape_like(search));
    let (visited_at, id) = before.map_or((i64::MAX, i64::MAX), |cursor| {
        (cursor.visited_at, cursor.id)
    });
    // One more row than asked says whether another page follows.
    let mut statement = connection.prepare_cached(
        "SELECT id, url, title, visited_at FROM browser_visits
         WHERE (visited_at < ?1 OR (visited_at = ?1 AND id < ?2))
           AND (?3 = '%%' OR url LIKE ?3 ESCAPE '\\' OR title LIKE ?3 ESCAPE '\\')
         ORDER BY visited_at DESC, id DESC
         LIMIT ?4",
    )?;
    let mut visits = statement
        .query_map(params![visited_at, id, pattern, limit + 1], |row| {
            Ok(Visit {
                id: row.get(0)?,
                url: row.get(1)?,
                title: row.get(2)?,
                visited_at: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let next = if visits.len() > limit as usize {
        visits.truncate(limit as usize);
        visits.last().map(|visit| Cursor {
            visited_at: visit.visited_at,
            id: visit.id,
        })
    } else {
        None
    };
    Ok(VisitPage { visits, next })
}

pub(super) fn remove(connection: &Connection, ids: &[i64]) -> rusqlite::Result<usize> {
    if ids.is_empty() {
        return Ok(0);
    }
    let placeholders = vec!["?"; ids.len()].join(",");
    connection.execute(
        &format!("DELETE FROM browser_visits WHERE id IN ({placeholders})"),
        params_from_iter(ids),
    )
}

pub(super) fn remove_range(connection: &Connection, from: i64, to: i64) -> rusqlite::Result<usize> {
    connection.execute(
        "DELETE FROM browser_visits WHERE visited_at >= ?1 AND visited_at < ?2",
        params![from, to],
    )
}

fn escape_like(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for character in text.chars() {
        if matches!(character, '%' | '_' | '\\') {
            escaped.push('\\');
        }
        escaped.push(character);
    }
    escaped
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
