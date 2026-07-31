use rusqlite::params;

use crate::{
    editor_persistence::{MAX_DIRTY_BUFFERS, MAX_TOTAL_DIRTY_BYTES},
    error::{AppError, AppResult},
    project_registry::{MAX_PROJECTS, MAX_ROOTS_PER_PROJECT},
    workbench_persistence::MAX_RESTORED_TABS,
};

pub(super) const MAX_TOTAL_LEGACY_ROOTS: i64 = MAX_PROJECTS * MAX_ROOTS_PER_PROJECT;
pub(super) const MAX_TOTAL_LEGACY_BUFFERS: i64 = MAX_PROJECTS * MAX_DIRTY_BUFFERS;
pub(super) const MAX_TOTAL_LEGACY_TABS: i64 = MAX_PROJECTS * MAX_RESTORED_TABS as i64;
const MAX_LEGACY_ROOT_PATH_BYTES: i64 = 16 * 1024 * 1024;
const MAX_LEGACY_TAB_BYTES: i64 = 64 * 1024 * 1024;

pub(super) fn preflight_legacy_bounds(transaction: &rusqlite::Transaction<'_>) -> AppResult<()> {
    let project_count: i64 =
        transaction.query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?;
    if project_count > MAX_PROJECTS {
        return Err(AppError::ResourceLimit(format!(
            "legacy registry exceeds its {MAX_PROJECTS}-project limit"
        )));
    }
    let (root_count, root_bytes): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(length(CAST(absolute_path AS BLOB))), 0)
         FROM project_roots",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if root_count > MAX_TOTAL_LEGACY_ROOTS || root_bytes > MAX_LEGACY_ROOT_PATH_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy roots exceed migration count or byte limits".into(),
        ));
    }
    let oversized_roots: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM project_roots GROUP BY project_id HAVING COUNT(*) > ?1
         )",
        [MAX_ROOTS_PER_PROJECT],
        |row| row.get(0),
    )?;
    if oversized_roots {
        return Err(AppError::ResourceLimit(format!(
            "legacy project exceeds its {MAX_ROOTS_PER_PROJECT}-root limit"
        )));
    }
    let (tab_count, tab_bytes): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*), COALESCE(SUM(
           length(CAST(title AS BLOB)) + length(CAST(metadata_json AS BLOB))
           + COALESCE(length(CAST(status AS BLOB)), 0)
         ), 0) FROM workbench_tabs",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if tab_count > MAX_TOTAL_LEGACY_TABS || tab_bytes > MAX_LEGACY_TAB_BYTES {
        return Err(AppError::ResourceLimit(
            "legacy tabs exceed migration count or byte limits".into(),
        ));
    }
    let oversized_tabs: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM workbench_tabs GROUP BY project_id HAVING COUNT(*) > ?1
         )",
        [MAX_RESTORED_TABS as i64],
        |row| row.get(0),
    )?;
    if oversized_tabs {
        return Err(AppError::ResourceLimit(format!(
            "legacy project exceeds its {MAX_RESTORED_TABS}-tab limit"
        )));
    }
    let (buffer_count, buffer_bytes): (i64, i64) = transaction.query_row(
        "SELECT COUNT(*),
                COALESCE(SUM(length(CAST(buffer_content AS BLOB))), 0)
         FROM editor_buffers",
        [],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    if buffer_count > MAX_TOTAL_LEGACY_BUFFERS
        || buffer_bytes > MAX_PROJECTS * MAX_TOTAL_DIRTY_BYTES
    {
        return Err(AppError::ResourceLimit(
            "legacy dirty buffers exceed aggregate migration limits".into(),
        ));
    }
    let oversized_buffers: bool = transaction.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM editor_buffers GROUP BY project_id
           HAVING COUNT(*) > ?1
              OR SUM(length(CAST(buffer_content AS BLOB))) > ?2
         )",
        params![MAX_DIRTY_BUFFERS, MAX_TOTAL_DIRTY_BYTES],
        |row| row.get(0),
    )?;
    if oversized_buffers {
        return Err(AppError::ResourceLimit(
            "legacy project exceeds dirty-buffer recovery limits".into(),
        ));
    }
    Ok(())
}
