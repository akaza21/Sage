use std::path::{Component, PathBuf};

use rusqlite::{Connection, params};
use sage_core::error::{AppError, AppResult};
use sage_db::error::SqliteResultExt;

use crate::{
    editor_persistence::{MAX_TOTAL_DIRTY_BYTES, MAX_TOTAL_DIRTY_METADATA_BYTES},
    project_registry::{RootRecord, canonical_comparison_key},
};
use sage_workspace::validate_contract_path;

const PROJECT_BYTES: i64 = 2 * 1024 * 1024;
const ROOT_BYTES: i64 = 16 * 1024 * 1024;
const TAB_BYTES: i64 = 64 * 1024 * 1024;
const PROJECT_LIMIT: i64 = 64;

pub(super) fn preflight(connection: &Connection) -> AppResult<()> {
    let invalid_scalar: bool = connection
        .query_row(
            "SELECT EXISTS(
           SELECT 1 FROM projects
           WHERE length(CAST(id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(name AS BLOB)) NOT BETWEEN 1 AND 1024
           UNION ALL
           SELECT 1 FROM project_roots
           WHERE length(CAST(project_id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(absolute_path AS BLOB)) NOT BETWEEN 1 AND 32768
           UNION ALL
           SELECT 1 FROM workbench_tabs
           WHERE length(CAST(id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(project_id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(title AS BLOB)) NOT BETWEEN 1 AND 1024
              OR length(CAST(metadata_json AS BLOB)) > 65536
           LIMIT 1
         )",
            [],
            |row| row.get(0),
        )
        .db()?;
    if invalid_scalar {
        return limit("legacy project metadata violates scalar byte limits");
    }
    aggregate_bound(
        connection,
        "SELECT COALESCE(SUM(length(CAST(id AS BLOB)) + length(CAST(name AS BLOB))),0) FROM projects",
        PROJECT_BYTES,
        "legacy projects exceed their aggregate byte limit",
    )?;
    aggregate_bound(
        connection,
        "SELECT COALESCE(SUM(length(CAST(project_id AS BLOB)) + length(CAST(absolute_path AS BLOB))),0) FROM project_roots",
        ROOT_BYTES,
        "legacy roots exceed their aggregate byte limit",
    )?;
    aggregate_bound(
        connection,
        "SELECT COALESCE(SUM(length(CAST(id AS BLOB)) + length(CAST(project_id AS BLOB)) + length(CAST(kind AS BLOB)) + length(CAST(title AS BLOB)) + COALESCE(length(CAST(status AS BLOB)),0) + length(CAST(metadata_json AS BLOB))),0) FROM workbench_tabs",
        TAB_BYTES,
        "legacy tabs exceed their aggregate byte limit",
    )?;
    if table_exists(connection, "editor_buffers")? {
        let (content, metadata): (i64, i64) = connection
            .query_row(
                "SELECT COALESCE(SUM(length(CAST(buffer_content AS BLOB))),0),
                    COALESCE(SUM(length(CAST(project_id AS BLOB))
                      + length(CAST(relative_path AS BLOB))
                      + length(CAST(base_fingerprint AS BLOB))
                      + length(CAST(line_ending AS BLOB))
                      + length(CAST(buffer_fingerprint AS BLOB))
                      + COALESCE(length(CAST(pending_temp_path AS BLOB)),0)),0)
             FROM editor_buffers",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .db()?;
        if content > PROJECT_LIMIT * MAX_TOTAL_DIRTY_BYTES
            || metadata > PROJECT_LIMIT * MAX_TOTAL_DIRTY_METADATA_BYTES
        {
            return limit("legacy dirty buffers exceed aggregate byte limits");
        }
    }
    if table_has_column(connection, "project_roots", "id")? {
        validate_v4_ownership(connection)?;
    }
    Ok(())
}

fn validate_v4_ownership(connection: &Connection) -> AppResult<()> {
    let invalid: bool = connection
        .query_row(
            "SELECT EXISTS(
           SELECT 1 FROM project_roots r LEFT JOIN projects p ON p.id=r.project_id
           WHERE p.id IS NULL OR length(CAST(r.id AS BLOB)) NOT BETWEEN 1 AND 512
              OR length(CAST(r.absolute_path AS BLOB))=0
              OR length(CAST(r.canonical_key AS BLOB))=0
           UNION ALL
           SELECT 1 FROM workbench_tabs t LEFT JOIN projects p ON p.id=t.project_id
           WHERE p.id IS NULL OR json_valid(t.metadata_json)=0
           UNION ALL
           SELECT 1 FROM editor_buffers b
           LEFT JOIN project_roots r ON r.id=b.root_id AND r.project_id=b.project_id
           WHERE r.id IS NULL OR b.buffer_byte_len <> length(CAST(b.buffer_content AS BLOB))
           LIMIT 1
         )",
            [],
            |row| row.get(0),
        )
        .db()?;
    if invalid {
        return invalid_request("legacy project/resource ownership metadata is corrupt");
    }

    let mut projects = connection
        .prepare("SELECT id FROM projects ORDER BY id LIMIT ?1")
        .db()?;
    let ids = projects
        .query_map([PROJECT_LIMIT + 1], |row| row.get::<_, String>(0))
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    if ids.len() > PROJECT_LIMIT as usize {
        return limit("legacy project count exceeds its limit");
    }
    for project_id in ids {
        let roots = root_page(connection, &project_id)?;
        validate_root_keys(connection, &roots)?;
        validate_buffer_paths(connection, &project_id)?;
    }
    Ok(())
}

fn validate_root_keys(connection: &Connection, roots: &[RootRecord]) -> AppResult<()> {
    for root in roots {
        if !root.absolute_path.is_absolute() {
            return invalid_request("legacy project root is not absolute");
        }
        match root.absolute_path.canonicalize() {
            Ok(canonical) if canonical != root.absolute_path => {
                return invalid_request("legacy project root spelling is not canonical");
            }
            Ok(_) => {}
            Err(_) => {
                if root
                    .absolute_path
                    .components()
                    .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
                    || root.absolute_path.components().collect::<PathBuf>() != root.absolute_path
                {
                    return invalid_request("missing legacy project root is not normalized");
                }
            }
        }
        let stored: String = connection
            .query_row(
                "SELECT canonical_key FROM project_roots WHERE id=?1 AND project_id=?2",
                params![root.id, root.project_id],
                |row| row.get(0),
            )
            .db()?;
        if stored != canonical_comparison_key(&root.absolute_path) {
            return invalid_request("legacy project root canonical identity is corrupt");
        }
    }
    Ok(())
}

fn root_page(connection: &Connection, project_id: &str) -> AppResult<Vec<RootRecord>> {
    let mut statement = connection
        .prepare(
            "SELECT id, project_id, absolute_path, ordinal FROM project_roots
         WHERE project_id=?1 ORDER BY ordinal LIMIT 9",
        )
        .db()?;
    let roots = statement
        .query_map([project_id], |row| {
            Ok(RootRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                absolute_path: PathBuf::from(row.get::<_, String>(2)?),
                ordinal: row.get(3)?,
            })
        })
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    if roots.is_empty() || roots.len() > 8 {
        return limit("legacy project has an invalid bounded root count");
    }
    Ok(roots)
}

fn validate_buffer_paths(connection: &Connection, project_id: &str) -> AppResult<()> {
    let mut statement = connection
        .prepare(
            "SELECT relative_path FROM editor_buffers WHERE project_id=?1
         ORDER BY root_id, relative_path LIMIT 17",
        )
        .db()?;
    let paths = statement
        .query_map(params![project_id], |row| row.get::<_, String>(0))
        .db()?
        .collect::<Result<Vec<_>, _>>()
        .db()?;
    if paths.len() > 16 {
        return limit("legacy project exceeds its dirty-buffer count limit");
    }
    for path in paths {
        validate_contract_path(&path, false)?;
    }
    Ok(())
}

fn aggregate_bound(
    connection: &Connection,
    sql: &str,
    maximum: i64,
    message: &str,
) -> AppResult<()> {
    let bytes: i64 = connection.query_row(sql, [], |row| row.get(0)).db()?;
    if bytes > maximum {
        return limit(message);
    }
    Ok(())
}

fn table_exists(connection: &Connection, name: &str) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE type='table' AND name=?1)",
            [name],
            |row| row.get(0),
        )
        .db()
}

fn table_has_column(connection: &Connection, table: &str, column: &str) -> AppResult<bool> {
    connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM pragma_table_xinfo(?1) WHERE name=?2)",
            params![table, column],
            |row| row.get(0),
        )
        .db()
}

fn invalid_request<T>(message: &str) -> AppResult<T> {
    Err(AppError::InvalidRequest(message.into()))
}

fn limit<T>(message: &str) -> AppResult<T> {
    Err(AppError::ResourceLimit(message.into()))
}
