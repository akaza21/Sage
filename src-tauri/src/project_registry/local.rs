use rusqlite::{Connection, OptionalExtension, params};
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use sage_db::error::SqliteResultExt;
use uuid::Uuid;

use sage_db::database::Database;
use sage_domain::project::ProjectSummary;

use super::seed::seed_project;
use super::{MAX_PROJECTS, project_summary_in};

pub(super) fn ensure_local_project(database: &Database) -> AppResult<ProjectSummary> {
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction().db()?;
    if let Some(existing_id) = existing_local_project_id(&transaction)? {
        let now = now_millis();
        transaction
            .execute(
                "UPDATE projects SET last_opened_at = ?2, updated_at = ?2 WHERE id = ?1",
                params![existing_id, now],
            )
            .db()?;
        transaction
            .execute(
                "INSERT INTO app_state(singleton, active_project_id, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               active_project_id = excluded.active_project_id,
               updated_at = excluded.updated_at",
                params![existing_id, now],
            )
            .db()?;
        let summary = project_summary_in(&transaction, &existing_id)?;
        transaction.commit().db()?;
        return Ok(summary);
    }
    let count: i64 = transaction
        .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))
        .db()?;
    if count >= MAX_PROJECTS {
        return Err(AppError::ResourceLimit(format!(
            "project registry reached its {MAX_PROJECTS}-project limit"
        )));
    }
    let project_id = Uuid::new_v4().to_string();
    let tab_id = Uuid::new_v4().to_string();
    let now = now_millis();
    transaction
        .execute(
            "INSERT INTO projects(id, name, created_at, updated_at, last_opened_at)
         VALUES (?1, 'Local', ?2, ?2, ?2)",
            params![project_id, now],
        )
        .db()?;
    seed_project(&transaction, &project_id, &tab_id, None, now)?;
    let summary = project_summary_in(&transaction, &project_id)?;
    transaction.commit().db()?;
    Ok(summary)
}

fn existing_local_project_id(connection: &Connection) -> AppResult<Option<String>> {
    connection
        .query_row(
            "SELECT p.id FROM projects p
             WHERE NOT EXISTS (
               SELECT 1 FROM project_roots r WHERE r.project_id = p.id
             )
             ORDER BY CASE WHEN p.name = 'Local' THEN 0 ELSE 1 END, p.created_at, p.id
             LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .db()
}
