mod create;
mod launch_root;
mod local;
mod rail;
pub(crate) use rail::{CHAT_FINISHED_AT, CHAT_LATEST_TURN_ID};
mod seed;

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension, params};
use uuid::Uuid;

use crate::{
    acp::contract::SageRootId,
    domain::{ProjectListItem, ProjectRootSummary, ProjectSummary, ResourceRef},
    error::{AppError, AppResult},
    persistence::{Database, now_millis},
    state::ActiveProjectContext,
    workspace::Workspace,
};

pub use create::project_name;
pub(crate) use launch_root::ProjectRootAuthority;

use self::rail::load_project_list;
use self::seed::{active_project_in, insert_root};

pub const MAX_PROJECTS: i64 = 64;
pub const MAX_ROOTS_PER_PROJECT: i64 = 8;

#[derive(Debug, Clone)]
pub struct RootRecord {
    pub id: String,
    pub project_id: String,
    pub absolute_path: PathBuf,
    pub ordinal: i64,
}

pub fn canonical_comparison_key(path: &Path) -> String {
    let value = path.to_string_lossy();
    if cfg!(windows) {
        value.replace('/', "\\").to_lowercase()
    } else {
        value.into_owned()
    }
}

pub(crate) fn persisted_path_text(path: &Path) -> AppResult<&str> {
    path.to_str().ok_or_else(|| {
        AppError::InvalidRequest(
            "selected project root is not valid UTF-8 and cannot be persisted".into(),
        )
    })
}

impl Database {
    pub(crate) fn selected_project_root_authority(
        &self,
        context: &ActiveProjectContext<'_>,
    ) -> AppResult<ProjectRootAuthority> {
        let root_id = self
            .connection()
            .query_row(
                "SELECT selected_root_id FROM project_layouts WHERE project_id = ?1",
                [context.project_id().as_str()],
                |row| row.get::<_, Option<String>>(0),
            )?
            .ok_or_else(|| {
                AppError::InvalidRequest("the active project has no selected root".into())
            })?;
        let root_id = SageRootId::parse(root_id).map_err(|_| {
            AppError::InvalidRequest("selected project root identity is invalid".into())
        })?;
        self.project_root_authority(context, &root_id)
    }

    pub(crate) fn project_root_authority(
        &self,
        context: &ActiveProjectContext<'_>,
        root_id: &SageRootId,
    ) -> AppResult<ProjectRootAuthority> {
        if self.active_project_id()?.as_deref() != Some(context.project_id().as_str()) {
            return Err(AppError::InvalidRequest(
                "project changed while resolving the process root".into(),
            ));
        }
        let record = self.root_record(root_id.as_str())?;
        if record.project_id != context.project_id().as_str() {
            return Err(AppError::InvalidRequest(
                "selected root does not belong to the active project".into(),
            ));
        }
        let canonical = record.absolute_path.canonicalize().map_err(|_| {
            AppError::InvalidRequest("selected project root is missing or unavailable".into())
        })?;
        if canonical.as_os_str() != record.absolute_path.as_os_str() || !canonical.is_dir() {
            return Err(AppError::InvalidRequest(
                "registered project root is not its canonical directory identity".into(),
            ));
        }
        let workspace = Workspace::open(&record.project_id, &record.id, &record.absolute_path)?;
        ProjectRootAuthority::acquire(
            workspace,
            context.project_id().clone(),
            context.generation(),
            root_id.clone(),
        )
    }

    pub fn project_count(&self) -> AppResult<i64> {
        Ok(self
            .connection()
            .query_row("SELECT COUNT(*) FROM projects", [], |row| row.get(0))?)
    }

    pub fn project_list(&self) -> AppResult<Vec<ProjectListItem>> {
        load_project_list(&self.connection())
    }

    pub fn active_project_id(&self) -> AppResult<Option<String>> {
        Ok(self
            .connection()
            .query_row(
                "SELECT active_project_id FROM app_state WHERE singleton = 1",
                [],
                |row| row.get(0),
            )
            .optional()?
            .flatten())
    }

    pub fn repair_active_project(&self) -> AppResult<Option<String>> {
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let configured = transaction
            .query_row(
                "SELECT active_project_id FROM app_state WHERE singleton = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
        let configured_exists = configured.as_ref().is_some_and(|project_id| {
            transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM projects p WHERE p.id = ?1)",
                    [project_id],
                    |row| row.get::<_, bool>(0),
                )
                .unwrap_or(false)
        });
        let active = if configured_exists {
            configured
        } else {
            transaction
                .query_row(
                    "SELECT p.id FROM projects p
                     ORDER BY p.last_opened_at DESC, p.id
                     LIMIT 1",
                    [],
                    |row| row.get::<_, String>(0),
                )
                .optional()?
        };
        transaction.execute(
            "INSERT INTO app_state(singleton, active_project_id, updated_at)
             VALUES (1, ?1, ?2)
             ON CONFLICT(singleton) DO UPDATE SET
               active_project_id = excluded.active_project_id,
               updated_at = excluded.updated_at",
            params![active, now_millis()],
        )?;
        transaction.commit()?;
        Ok(active)
    }

    pub fn project_summary(&self, project_id: &str) -> AppResult<ProjectSummary> {
        project_summary_in(&self.connection(), project_id)
    }

    pub fn root_records(&self, project_id: &str) -> AppResult<Vec<RootRecord>> {
        root_records_in(&self.connection(), project_id)
    }

    pub fn resolve_active_resource(&self, resource: &ResourceRef) -> AppResult<Workspace> {
        let active = self
            .active_project_id()?
            .ok_or_else(|| AppError::InvalidRequest("no project is active".into()))?;
        let root = self.root_record(&resource.root_id)?;
        if root.project_id != active {
            return Err(AppError::InvalidRequest(
                "resource root does not belong to the active project".into(),
            ));
        }
        Workspace::open(&root.project_id, &root.id, &root.absolute_path)
    }

    pub fn root_record(&self, root_id: &str) -> AppResult<RootRecord> {
        self.connection()
            .query_row(
                "SELECT id, project_id, absolute_path, ordinal
                 FROM project_roots WHERE id = ?1",
                [root_id],
                |row| {
                    Ok(RootRecord {
                        id: row.get(0)?,
                        project_id: row.get(1)?,
                        absolute_path: PathBuf::from(row.get::<_, String>(2)?),
                        ordinal: row.get(3)?,
                    })
                },
            )
            .optional()?
            .ok_or_else(|| AppError::InvalidRequest("unknown project root".into()))
    }

    pub fn ensure_local_project(&self) -> AppResult<ProjectSummary> {
        local::ensure_local_project(self)
    }

    pub fn add_project_root(&self, project_id: &str, root: &Path) -> AppResult<ProjectSummary> {
        let canonical = canonical_folder(root)?;
        let canonical_key = canonical_comparison_key(&canonical);
        let mut connection = self.write_connection()?;
        let transaction = connection.transaction()?;
        let active = active_project_in(&transaction)?;
        if active.as_deref() != Some(project_id) {
            return Err(AppError::InvalidRequest(
                "roots can only be added to the active project".into(),
            ));
        }
        let count: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM project_roots WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )?;
        if count >= MAX_ROOTS_PER_PROJECT {
            return Err(AppError::ResourceLimit(format!(
                "project reached its {MAX_ROOTS_PER_PROJECT}-root limit"
            )));
        }
        let duplicate = transaction.query_row(
            "SELECT EXISTS(
               SELECT 1 FROM project_roots
               WHERE project_id = ?1 AND canonical_key = ?2
             )",
            params![project_id, canonical_key],
            |row| row.get::<_, bool>(0),
        )?;
        if duplicate {
            return Err(AppError::InvalidRequest(
                "this canonical folder is already a project root".into(),
            ));
        }
        let root_id = Uuid::new_v4().to_string();
        insert_root(&transaction, project_id, &root_id, &canonical, count)?;
        transaction.execute(
            "UPDATE projects SET updated_at = ?2 WHERE id = ?1",
            params![project_id, now_millis()],
        )?;
        let summary = project_summary_in(&transaction, project_id)?;
        transaction.commit()?;
        Ok(summary)
    }
}

pub(super) fn project_summary_in(
    connection: &Connection,
    project_id: &str,
) -> AppResult<ProjectSummary> {
    let name = connection.query_row(
        "SELECT name FROM projects WHERE id = ?1",
        [project_id],
        |row| row.get::<_, String>(0),
    )?;
    let roots = root_records_in(connection, project_id)?
        .into_iter()
        .map(|root| root_summary(&root))
        .collect();
    Ok(ProjectSummary {
        id: project_id.into(),
        name,
        roots,
    })
}

fn root_records_in(connection: &Connection, project_id: &str) -> AppResult<Vec<RootRecord>> {
    let mut statement = connection.prepare(
        "SELECT id, project_id, absolute_path, ordinal
         FROM project_roots WHERE project_id = ?1
         ORDER BY ordinal LIMIT ?2",
    )?;
    let roots = statement
        .query_map(params![project_id, MAX_ROOTS_PER_PROJECT + 1], |row| {
            Ok(RootRecord {
                id: row.get(0)?,
                project_id: row.get(1)?,
                absolute_path: PathBuf::from(row.get::<_, String>(2)?),
                ordinal: row.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if roots.len() > MAX_ROOTS_PER_PROJECT as usize {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_ROOTS_PER_PROJECT}-root limit"
        )));
    }
    Ok(roots)
}

fn canonical_folder(root: &Path) -> AppResult<PathBuf> {
    let canonical = root.canonicalize()?;
    if !canonical.is_dir() {
        return Err(AppError::InvalidRequest(
            "selected project root is not a directory".into(),
        ));
    }
    persisted_path_text(&canonical)?;
    Ok(canonical)
}

fn folder_name(root: &Path) -> String {
    root.file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty())
        .unwrap_or("Project")
        .to_owned()
}

fn root_summary(root: &RootRecord) -> ProjectRootSummary {
    let (availability, message) = match std::fs::metadata(&root.absolute_path) {
        Ok(metadata) if metadata.is_dir() => ("available", None),
        Ok(_) => ("invalid", Some("Root path is not a directory".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            ("missing", Some("Root folder is missing or offline".into()))
        }
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => (
            "permissionDenied",
            Some("Permission to this root was denied".into()),
        ),
        Err(error) => ("unavailable", Some(error.to_string())),
    };
    ProjectRootSummary {
        id: root.id.clone(),
        name: folder_name(&root.absolute_path),
        ordinal: root.ordinal as u32,
        availability: availability.into(),
        message,
    }
}

#[cfg(test)]
mod tests;
