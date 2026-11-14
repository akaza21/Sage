use rusqlite::params;
use sage_core::error::{AppError, AppResult};

use crate::domain::{RepositoryRef, ResourceRef};

use super::validate_resource_for_project;

pub fn validate_layout_tab_repository(
    connection: &rusqlite::Connection,
    project_id: &str,
    kind: &str,
    resource: Option<&ResourceRef>,
    repository: Option<&RepositoryRef>,
) -> AppResult<()> {
    validate_tab_repository(kind, resource, repository)?;
    if let Some(repository) = repository {
        if repository.relative_path.is_empty() {
            validate_root_membership(connection, project_id, &repository.root_id)?;
        } else {
            validate_resource_for_project(
                connection,
                project_id,
                &ResourceRef {
                    root_id: repository.root_id.clone(),
                    relative_path: repository.relative_path.clone(),
                },
            )?;
        }
    }
    Ok(())
}

pub fn validate_tab_repository(
    kind: &str,
    resource: Option<&ResourceRef>,
    repository: Option<&RepositoryRef>,
) -> AppResult<()> {
    let Some(repository) = repository else {
        return Ok(());
    };
    if kind != "diff" {
        return Err(AppError::InvalidRequest(
            "only persisted Diff tabs may carry a repository".into(),
        ));
    }
    let Some(resource) = resource else {
        return Err(AppError::InvalidRequest(
            "nested Diff tabs require a resource".into(),
        ));
    };
    if resource.root_id != repository.root_id {
        return Err(AppError::InvalidRequest(
            "repository root does not match the Diff resource root".into(),
        ));
    }
    Ok(())
}

pub fn validate_root_membership(
    connection: &rusqlite::Connection,
    project_id: &str,
    root_id: &str,
) -> AppResult<()> {
    let matches: bool = connection.query_row(
        "SELECT EXISTS(
           SELECT 1 FROM project_roots WHERE id = ?1 AND project_id = ?2
         )",
        params![root_id, project_id],
        |row| row.get(0),
    )?;
    if !matches {
        return Err(AppError::InvalidRequest(
            "layout resource root does not belong to its project".into(),
        ));
    }
    Ok(())
}
