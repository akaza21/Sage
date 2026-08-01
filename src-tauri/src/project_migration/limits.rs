use rusqlite::params;

use crate::{
    editor_persistence::{
        MAX_DIRTY_BUFFERS, MAX_TOTAL_DIRTY_BYTES, MAX_TOTAL_DIRTY_METADATA_BYTES,
    },
    error::{AppError, AppResult},
    project_registry::{MAX_PROJECTS, MAX_ROOTS_PER_PROJECT},
    workbench_persistence::MAX_RESTORED_TABS,
};

pub(super) const MAX_TOTAL_LEGACY_ROOTS: i64 = MAX_PROJECTS * MAX_ROOTS_PER_PROJECT;
pub(super) const MAX_TOTAL_LEGACY_BUFFERS: i64 = MAX_PROJECTS * MAX_DIRTY_BUFFERS;
pub(super) const MAX_TOTAL_LEGACY_TABS: i64 = MAX_PROJECTS * MAX_RESTORED_TABS as i64;
const MAX_LEGACY_PROJECT_BYTES: i64 = 2 * 1024 * 1024;
const MAX_LEGACY_ROOT_BYTES: i64 = 16 * 1024 * 1024;
const MAX_LEGACY_TAB_BYTES: i64 = 64 * 1024 * 1024;

pub(super) fn preflight_legacy_bounds(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    preflight_projects(transaction)?;
    preflight_roots(transaction)?;
    preflight_tabs(transaction)?;
    preflight_buffers(transaction)
}

fn preflight_projects(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let count = limited_count(
        transaction,
        "SELECT COUNT(*) FROM (SELECT 1 FROM projects LIMIT ?1)",
        MAX_PROJECTS,
    )?;
    if count > MAX_PROJECTS {
        return Err(AppError::ResourceLimit(format!(
            "legacy registry exceeds its {MAX_PROJECTS}-project limit"
        )));
    }
    let bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(
           length(CAST(id AS BLOB)) + length(CAST(name AS BLOB))
         ), 0) FROM projects",
        [],
        |row| row.get(0),
    )?;
    if bytes > MAX_LEGACY_PROJECT_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy projects exceed migration byte limits".into(),
        ));
    }
    Ok(())
}

fn preflight_roots(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let count = limited_count(
        transaction,
        "SELECT COUNT(*) FROM (SELECT 1 FROM project_roots LIMIT ?1)",
        MAX_TOTAL_LEGACY_ROOTS,
    )?;
    if count > MAX_TOTAL_LEGACY_ROOTS {
        return Err(AppError::ResourceLimit(
            "legacy roots exceed migration count limits".into(),
        ));
    }
    let bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(
           length(CAST(project_id AS BLOB))
           + length(CAST(absolute_path AS BLOB))
         ), 0) FROM project_roots",
        [],
        |row| row.get(0),
    )?;
    if bytes > MAX_LEGACY_ROOT_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy roots exceed migration byte limits".into(),
        ));
    }
    let oversized: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM project_roots GROUP BY project_id HAVING COUNT(*) > ?1
         )",
        [MAX_ROOTS_PER_PROJECT],
        |row| row.get(0),
    )?;
    if oversized {
        return Err(AppError::ResourceLimit(format!(
            "legacy project exceeds its {MAX_ROOTS_PER_PROJECT}-root limit"
        )));
    }
    Ok(())
}

fn preflight_tabs(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let count = limited_count(
        transaction,
        "SELECT COUNT(*) FROM (SELECT 1 FROM workbench_tabs LIMIT ?1)",
        MAX_TOTAL_LEGACY_TABS,
    )?;
    if count > MAX_TOTAL_LEGACY_TABS {
        return Err(AppError::ResourceLimit(
            "legacy tabs exceed migration count limits".into(),
        ));
    }
    let bytes: i64 = transaction.query_row(
        "SELECT COALESCE(SUM(
           length(CAST(id AS BLOB)) + length(CAST(project_id AS BLOB))
           + length(CAST(kind AS BLOB)) + length(CAST(title AS BLOB))
           + COALESCE(length(CAST(status AS BLOB)), 0)
           + length(CAST(metadata_json AS BLOB))
         ), 0) FROM workbench_tabs",
        [],
        |row| row.get(0),
    )?;
    if bytes > MAX_LEGACY_TAB_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy tabs exceed migration byte limits".into(),
        ));
    }
    let oversized: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM workbench_tabs GROUP BY project_id HAVING COUNT(*) > ?1
         )",
        [MAX_RESTORED_TABS as i64],
        |row| row.get(0),
    )?;
    if oversized {
        return Err(AppError::ResourceLimit(format!(
            "legacy project exceeds its {MAX_RESTORED_TABS}-tab limit"
        )));
    }
    Ok(())
}

fn preflight_buffers(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let count = limited_count(
        transaction,
        "SELECT COUNT(*) FROM (SELECT 1 FROM editor_buffers LIMIT ?1)",
        MAX_TOTAL_LEGACY_BUFFERS,
    )?;
    if count > MAX_TOTAL_LEGACY_BUFFERS {
        return Err(AppError::ResourceLimit(
            "legacy dirty-buffer count exceeds aggregate migration limits".into(),
        ));
    }
    let (content_bytes, metadata_bytes): (i64, i64) = transaction.query_row(
        "SELECT COALESCE(SUM(length(CAST(buffer_content AS BLOB))), 0),
                COALESCE(SUM(
           length(CAST(project_id AS BLOB))
           + length(CAST(relative_path AS BLOB))
           + length(CAST(base_fingerprint AS BLOB))
           + length(CAST(line_ending AS BLOB))
           + length(CAST(buffer_fingerprint AS BLOB))
           + COALESCE(length(CAST(pending_temp_path AS BLOB)), 0)
         ), 0) FROM editor_buffers",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if content_bytes > MAX_PROJECTS * MAX_TOTAL_DIRTY_BYTES
        || metadata_bytes > MAX_PROJECTS * MAX_TOTAL_DIRTY_METADATA_BYTES
    {
        return Err(AppError::ResourceLimit(
            "legacy dirty buffers exceed aggregate migration byte limits".into(),
        ));
    }
    let oversized: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers GROUP BY project_id
           HAVING COUNT(*) > ?1
              OR SUM(length(CAST(buffer_content AS BLOB))) > ?2
              OR SUM(
                length(CAST(project_id AS BLOB))
                + length(CAST(relative_path AS BLOB))
                + length(CAST(base_fingerprint AS BLOB))
                + length(CAST(line_ending AS BLOB))
                + length(CAST(buffer_fingerprint AS BLOB))
                + COALESCE(length(CAST(pending_temp_path AS BLOB)), 0)
              ) > ?3
         )",
        params![
            MAX_DIRTY_BUFFERS,
            MAX_TOTAL_DIRTY_BYTES,
            MAX_TOTAL_DIRTY_METADATA_BYTES
        ],
        |row| row.get(0),
    )?;
    if oversized {
        return Err(AppError::ResourceLimit(
            "legacy project exceeds dirty-buffer recovery limits".into(),
        ));
    }
    Ok(())
}

fn limited_count(transaction: &rusqlite::Transaction<'_>, sql: &str, limit: i64) -> AppResult<i64> {
    Ok(transaction.query_row(sql, [limit + 1], |row| row.get(0))?)
}
