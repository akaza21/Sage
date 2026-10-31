//! The browsing-history SQLite file (BF-7c). One row per visit: the page a
//! browser tab committed, when, and the title the page later reported.
//!
//! Every read and delete is keyed by `visited_at`, so clearing a time range
//! is one indexed `DELETE`; clearing everything is the widest range. The
//! table is bounded by count and age, and every value is checked by the
//! schema as well as by the writer.

use rusqlite::{Connection, OptionalExtension, Transaction, params, params_from_iter};
use serde::{Deserialize, Serialize};

/// The most visits kept. The oldest go first once a write passes it.
pub(super) const MAX_VISITS: i64 = 10_000;
/// Visits older than this are pruned on write, as Chrome keeps 90 days.
pub(super) const RETENTION_MS: i64 = 90 * 24 * 60 * 60 * 1000;
/// The longest stored URL; it matches browser admission.
pub(super) const MAX_URL_BYTES: usize = super::super::admission::MAX_BROWSER_URL_BYTES;
/// The longest stored title, in characters.
pub(super) const MAX_TITLE_CHARS: usize = 256;
/// The most rows one page read returns.
pub(crate) const MAX_PAGE: u32 = 100;
/// The longest search text, in characters.
pub(crate) const MAX_SEARCH_CHARS: usize = 256;
/// The most visits one remove names.
pub(crate) const MAX_REMOVE: usize = 100;

/// One visit as the History page shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Visit {
    pub id: i64,
    pub url: String,
    pub title: Option<String>,
    /// Milliseconds since the Unix epoch.
    pub visited_at: i64,
}

/// Where the next page of an ordered read starts: strictly older than this.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Cursor {
    pub visited_at: i64,
    pub id: i64,
}

/// A page of visits, newest first, and where the next one starts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct VisitPage {
    pub visits: Vec<Visit>,
    pub next: Option<Cursor>,
}

/// A visit waiting to be written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct NewVisit {
    pub url: String,
    pub visited_at: i64,
}

pub(super) fn open(path: &str) -> rusqlite::Result<Connection> {
    let connection = Connection::open(path)?;
    connection.pragma_update(None, "journal_mode", "WAL")?;
    // Losing the last few visits on power loss is acceptable; an fsync on
    // every page load is not.
    connection.pragma_update(None, "synchronous", "NORMAL")?;
    connection.pragma_update(None, "busy_timeout", 5_000)?;
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS browser_visits (
           id INTEGER PRIMARY KEY,
           url TEXT NOT NULL CHECK (length(url) BETWEEN 1 AND 2048),
           title TEXT CHECK (title IS NULL OR length(title) BETWEEN 1 AND 256),
           visited_at INTEGER NOT NULL CHECK (visited_at >= 0)
         ) STRICT;
         CREATE INDEX IF NOT EXISTS browser_visits_by_time
           ON browser_visits(visited_at);
         CREATE INDEX IF NOT EXISTS browser_visits_by_url
           ON browser_visits(url);",
    )?;
    Ok(connection)
}

/// Writes a batch of visits and title updates in one transaction, then
/// prunes past the count and age bounds. `now` is the newest time known to
/// the caller.
pub(super) fn write(
    connection: &mut Connection,
    batch: &[super::Write],
    now: i64,
) -> rusqlite::Result<()> {
    let transaction = connection.transaction()?;
    for write in batch {
        match write {
            super::Write::Visit(visit) => insert(&transaction, visit)?,
            super::Write::Title { url, title } => set_title(&transaction, url, title)?,
        }
    }
    prune(&transaction, now)?;
    transaction.commit()
}

fn insert(transaction: &Transaction<'_>, visit: &NewVisit) -> rusqlite::Result<()> {
    if visit.url.is_empty() || visit.url.len() > MAX_URL_BYTES || visit.visited_at < 0 {
        return Ok(());
    }
    transaction
        .prepare_cached("INSERT INTO browser_visits(url, visited_at) VALUES (?1, ?2)")?
        .execute(params![visit.url, visit.visited_at])?;
    Ok(())
}

/// The newest visit to `url` takes the title the page reported after it
/// committed.
fn set_title(transaction: &Transaction<'_>, url: &str, title: &str) -> rusqlite::Result<()> {
    let title = title.chars().take(MAX_TITLE_CHARS).collect::<String>();
    let title = (!title.is_empty()).then_some(title);
    transaction
        .prepare_cached(
            "UPDATE browser_visits SET title = ?2
             WHERE id = (SELECT max(id) FROM browser_visits WHERE url = ?1)",
        )?
        .execute(params![url, title])?;
    Ok(())
}

fn prune(transaction: &Transaction<'_>, now: i64) -> rusqlite::Result<()> {
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

/// Visits newest first, optionally only those whose URL or title contains
/// `search` (ASCII case-insensitive), starting strictly after `before`.
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

/// Removes the named visits; ids that no longer exist are ignored.
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

/// Removes every visit with `from <= visited_at < to`.
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
