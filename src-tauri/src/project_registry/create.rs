use std::collections::HashSet;
use std::path::{Path, PathBuf};

use rusqlite::params;
use sage_core::{
    error::{AppError, AppResult},
    time::now_millis,
};
use sage_db::error::SqliteResultExt;
use uuid::Uuid;

use super::seed::{active_project_in, insert_root, seed_empty_project};
use super::{
    MAX_PROJECTS, MAX_ROOTS_PER_PROJECT, canonical_folder, conservative_overlap_key, folder_name,
    project_summary_in,
};
use sage_db::database::Database;
use sage_domain::project::ProjectSummary;

pub const MAX_PROJECT_NAME_CHARS: usize = 64;

pub fn project_name(name: &str) -> AppResult<&str> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::InvalidRequest("project name is empty".into()));
    }
    if name.chars().count() > MAX_PROJECT_NAME_CHARS {
        return Err(AppError::InvalidRequest(format!(
            "project name is longer than {MAX_PROJECT_NAME_CHARS} characters"
        )));
    }
    if name.chars().any(char::is_control) {
        return Err(AppError::InvalidRequest(
            "project name contains control characters".into(),
        ));
    }
    Ok(name)
}

fn canonical_sources(roots: &[PathBuf]) -> AppResult<Vec<PathBuf>> {
    if roots.len() > MAX_ROOTS_PER_PROJECT as usize {
        return Err(AppError::ResourceLimit(format!(
            "project reached its {MAX_ROOTS_PER_PROJECT}-root limit"
        )));
    }
    let mut seen = HashSet::with_capacity(roots.len());
    roots
        .iter()
        .map(|root| {
            let canonical = canonical_folder(root)?;
            if !seen.insert(conservative_overlap_key(&canonical)) {
                return Err(AppError::InvalidRequest(
                    "the same folder is listed twice".into(),
                ));
            }
            Ok(canonical)
        })
        .collect()
}

pub(crate) trait ProjectCreateStore {
    #[cfg(test)]
    fn create_project_with_chat(&self, root: &Path) -> AppResult<ProjectSummary>;

    fn create_project_from_root(&self, root: &Path) -> AppResult<ProjectSummary>;

    fn create_named_project_if_active(
        &self,
        name: &str,
        roots: &[PathBuf],
        expected_active_project_id: Option<&str>,
    ) -> AppResult<ProjectSummary>;
}

impl ProjectCreateStore for Database {
    #[cfg(test)]
    fn create_project_with_chat(&self, root: &Path) -> AppResult<ProjectSummary> {
        let project = self.create_project_from_root(root)?;
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction().db()?;
        crate::chat_lifecycle::insert_new_chat(&transaction, &project.id, true, None)?;
        transaction.commit().db()?;
        Ok(project)
    }

    fn create_project_from_root(&self, root: &Path) -> AppResult<ProjectSummary> {
        let canonical = canonical_folder(root)?;
        create_project_with_intent(self, &folder_name(&canonical), &[canonical], None)
    }

    fn create_named_project_if_active(
        &self,
        name: &str,
        roots: &[PathBuf],
        expected_active_project_id: Option<&str>,
    ) -> AppResult<ProjectSummary> {
        let name = project_name(name)?;
        create_project_with_intent(self, name, roots, Some(expected_active_project_id))
    }
}

fn create_project_with_intent(
    database: &Database,
    name: &str,
    roots: &[PathBuf],
    expected_active_project_id: Option<Option<&str>>,
) -> AppResult<ProjectSummary> {
    let roots = canonical_sources(roots)?;
    let _private_storage = crate::private_chat::lock_storage();
    for root in &roots {
        crate::private_chat::require_user_root(database, root)?;
    }
    let project_id = Uuid::new_v4().to_string();
    let now = now_millis();
    let mut connection = database.write_connection()?;
    let transaction = connection.transaction().db()?;
    if let Some(expected) = expected_active_project_id
        && active_project_in(&transaction)?.as_deref() != expected
    {
        return Err(AppError::InvalidRequest(
            "project changed while the project was being created".into(),
        ));
    }
    let count: i64 = transaction
        .query_row(
            "SELECT COUNT(*) FROM projects WHERE kind='user'",
            [],
            |row| row.get(0),
        )
        .db()?;
    if count >= MAX_PROJECTS {
        return Err(AppError::ResourceLimit(format!(
            "project registry reached its {MAX_PROJECTS}-project limit"
        )));
    }
    transaction
        .execute(
            "INSERT INTO projects(id, name, created_at, updated_at, last_opened_at)
             VALUES (?1, ?2, ?3, ?3, ?3)",
            params![project_id, name, now],
        )
        .db()?;
    let mut first_root_id = None;
    for (ordinal, root) in (0_i64..).zip(&roots) {
        let root_id = Uuid::new_v4().to_string();
        insert_root(&transaction, &project_id, &root_id, root, ordinal)?;
        first_root_id.get_or_insert(root_id);
    }
    seed_empty_project(&transaction, &project_id, first_root_id.as_deref(), now)?;
    let summary = project_summary_in(&transaction, &project_id)?;
    transaction.commit().db()?;
    Ok(summary)
}
