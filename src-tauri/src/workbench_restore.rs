use std::collections::{HashMap, HashSet};

use rusqlite::{OptionalExtension, params};

mod tab_repository;

use crate::{
    domain::{RepositoryRef, ResourceRef, WorkbenchTab},
    error::{AppError, AppResult},
    project_registry::RootRecord,
    workspace::{Workspace, validate_contract_path},
};

pub use tab_repository::{
    validate_layout_tab_repository, validate_root_membership, validate_tab_repository,
};

pub const MAX_RESTORED_TABS: usize = 64;
pub const MAX_RESOURCE_TABS: usize = 32;
pub const MAX_RESTORED_TAB_BYTES: i64 = 2 * 1024 * 1024;
const MAX_LAYOUT_SELECTION_BYTES: i64 = 64 * 1024;
const ALLOWED_TAB_KINDS: &[&str] = &[
    "chat", "file", "diff", "terminal", "search", "problems", "output", "settings",
];

pub struct RestoredLayout {
    pub tabs: Vec<WorkbenchTab>,
    pub active_tab_id: Option<String>,
    pub selected_root_id: Option<String>,
    pub selected_resource: Option<ResourceRef>,
}

pub fn restore_layout(
    connection: &rusqlite::Connection,
    project_id: &str,
    roots: &[RootRecord],
) -> AppResult<RestoredLayout> {
    let root_map = roots
        .iter()
        .map(|root| (root.id.as_str(), root))
        .collect::<HashMap<_, _>>();
    let tabs = load_tabs(connection, project_id, &root_map)?;
    let selection_bytes: i64 = connection
        .query_row(
            "SELECT COALESCE(
               length(CAST(active_tab_id AS BLOB)), 0
             ) + COALESCE(
               length(CAST(selected_root_id AS BLOB)), 0
             ) + COALESCE(
               length(CAST(selected_relative_path AS BLOB)), 0
             )
             FROM project_layouts WHERE project_id = ?1",
            [project_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    if selection_bytes > MAX_LAYOUT_SELECTION_BYTES {
        return Err(AppError::ResourceLimit(
            "persisted layout selection exceeds its byte limit".into(),
        ));
    }
    let (configured_tab, configured_root, selected_relative_path) = connection
        .query_row(
            "SELECT active_tab_id, selected_root_id, selected_relative_path
             FROM project_layouts WHERE project_id = ?1",
            [project_id],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, Option<String>>(1)?,
                    row.get::<_, Option<String>>(2)?,
                ))
            },
        )
        .optional()?
        .unwrap_or((None, None, None));
    let active_tab_id = configured_tab
        .filter(|id| tabs.iter().any(|tab| &tab.id == id))
        .or_else(|| tabs.first().map(|tab| tab.id.clone()));
    let configured_root_valid = configured_root
        .as_deref()
        .is_some_and(|root_id| root_map.contains_key(root_id));
    let selected_root_id = configured_root
        .filter(|root_id| root_map.contains_key(root_id.as_str()))
        .or_else(|| roots.first().map(|root| root.id.clone()));
    let selected_resource = if configured_root_valid {
        selected_root_id
            .as_ref()
            .zip(selected_relative_path)
            .and_then(|(root_id, relative_path)| {
                let resource = ResourceRef {
                    root_id: root_id.clone(),
                    relative_path,
                };
                validate_resource_metadata(root_map[root_id.as_str()], &resource)
                    .is_ok()
                    .then_some(resource)
            })
    } else {
        None
    };
    Ok(RestoredLayout {
        tabs,
        active_tab_id,
        selected_root_id,
        selected_resource,
    })
}

pub fn validate_resource_for_project(
    connection: &rusqlite::Connection,
    project_id: &str,
    resource: &ResourceRef,
) -> AppResult<()> {
    let root = connection
        .query_row(
            "SELECT id, project_id, absolute_path, ordinal
             FROM project_roots WHERE id = ?1 AND project_id = ?2",
            params![resource.root_id, project_id],
            |row| {
                Ok(RootRecord {
                    id: row.get(0)?,
                    project_id: row.get(1)?,
                    absolute_path: row.get::<_, String>(2)?.into(),
                    ordinal: row.get(3)?,
                })
            },
        )
        .optional()?
        .ok_or_else(|| {
            AppError::InvalidRequest("layout resource root does not belong to its project".into())
        })?;
    validate_resource_metadata(&root, resource)
}

fn load_tabs(
    connection: &rusqlite::Connection,
    project_id: &str,
    roots: &HashMap<&str, &RootRecord>,
) -> AppResult<Vec<WorkbenchTab>> {
    let count: i64 = connection.query_row(
        "SELECT COUNT(*) FROM (
           SELECT 1 FROM workbench_tabs
           WHERE project_id = ?1 LIMIT ?2
         )",
        params![project_id, MAX_RESTORED_TABS as i64 + 1],
        |row| row.get(0),
    )?;
    if count > MAX_RESTORED_TABS as i64 {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESTORED_TABS}-tab restore limit"
        )));
    }
    let bytes: i64 = connection.query_row(
        "SELECT COALESCE(SUM(
           length(CAST(id AS BLOB)) + length(CAST(kind AS BLOB))
           + length(CAST(title AS BLOB)) + length(CAST(metadata_json AS BLOB))
           + COALESCE(length(CAST(status AS BLOB)), 0)
         ), 0)
         FROM workbench_tabs WHERE project_id = ?1",
        [project_id],
        |row| row.get(0),
    )?;
    if bytes > MAX_RESTORED_TAB_BYTES {
        return Err(AppError::ResourceLimit(
            "persisted tabs exceed restore count or byte limits".into(),
        ));
    }
    let mut statement = connection.prepare(
        "SELECT w.id, w.kind, w.title, w.position, w.pinned, w.dirty, w.status, w.metadata_json,
                CASE
                  WHEN w.kind = 'chat' THEN COALESCE(c.updated_at, w.updated_at)
                  ELSE w.updated_at
                END
         FROM workbench_tabs w
         LEFT JOIN chat_tabs ct ON ct.project_id = w.project_id AND ct.tab_id = w.id
         LEFT JOIN chats c ON c.project_id = ct.project_id AND c.id = ct.chat_id
         WHERE w.project_id = ?1
         ORDER BY w.position LIMIT ?2",
    )?;
    let rows = statement
        .query_map(params![project_id, MAX_RESTORED_TABS as i64 + 1], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?,
                row.get::<_, i64>(4)? != 0,
                row.get::<_, i64>(5)? != 0,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, Option<i64>>(8)?,
            ))
        })?
        .collect::<Result<Vec<_>, _>>()?;
    if rows.len() > MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESTORED_TABS}-tab restore limit"
        )));
    }
    let mut ids = HashSet::new();
    let mut positions = HashSet::new();
    let tabs = rows
        .into_iter()
        .map(
            |(id, kind, title, position, pinned, dirty, status, metadata, updated_at)| {
                if !ids.insert(id.clone()) || !positions.insert(position) {
                    return Err(AppError::InvalidRequest(
                        "persisted layout contains duplicate tab identity or position".into(),
                    ));
                }
                if !ALLOWED_TAB_KINDS.contains(&kind.as_str()) {
                    return Err(AppError::InvalidRequest(format!(
                        "persisted tab has unsupported kind {kind}"
                    )));
                }
                let metadata: serde_json::Value = serde_json::from_str(&metadata)
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                let object = metadata.as_object().ok_or_else(|| {
                    AppError::InvalidRequest("persisted tab metadata is not an object".into())
                })?;
                if object.contains_key("resourcePath") {
                    return Err(AppError::InvalidRequest(
                        "persisted tab contains legacy resource metadata".into(),
                    ));
                }
                let resource = object
                    .get("resource")
                    .cloned()
                    .map(serde_json::from_value::<ResourceRef>)
                    .transpose()
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                let repository = object
                    .get("repository")
                    .cloned()
                    .map(serde_json::from_value::<RepositoryRef>)
                    .transpose()
                    .map_err(|error| AppError::InvalidRequest(error.to_string()))?;
                validate_tab_repository(&kind, resource.as_ref(), repository.as_ref())?;
                if let Some(repository) = &repository {
                    let root = roots.get(repository.root_id.as_str()).ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted tab repository root is foreign to its project".into(),
                        )
                    })?;
                    if !repository.relative_path.is_empty() {
                        validate_resource_metadata(
                            root,
                            &ResourceRef {
                                root_id: repository.root_id.clone(),
                                relative_path: repository.relative_path.clone(),
                            },
                        )?;
                    }
                }
                if kind == "file" || kind == "diff" {
                    let resource = resource.as_ref().ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted File and Diff tabs require a resource".into(),
                        )
                    })?;
                    let root = roots.get(resource.root_id.as_str()).ok_or_else(|| {
                        AppError::InvalidRequest(
                            "persisted tab resource root is foreign to its project".into(),
                        )
                    })?;
                    validate_resource_metadata(root, resource)?;
                } else if resource.is_some() {
                    return Err(AppError::InvalidRequest(
                        "only persisted File and Diff tabs may carry a resource".into(),
                    ));
                }
                Ok(WorkbenchTab {
                    id,
                    kind,
                    title,
                    position,
                    pinned,
                    dirty,
                    status,
                    updated_at: admit_tab_updated_at(updated_at),
                    resource,
                    repository,
                })
            },
        )
        .collect::<AppResult<Vec<_>>>()?;
    validate_tab_projection(&tabs)?;
    Ok(tabs)
}

pub fn validate_tab_projection(tabs: &[WorkbenchTab]) -> AppResult<()> {
    if tabs.len() > MAX_RESTORED_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESTORED_TABS}-tab restore limit"
        )));
    }
    let resource_count = tabs
        .iter()
        .filter(|tab| tab.kind == "file" || tab.kind == "diff")
        .count();
    if resource_count > MAX_RESOURCE_TABS {
        return Err(AppError::ResourceLimit(format!(
            "project exceeds its {MAX_RESOURCE_TABS}-resource-tab restore limit"
        )));
    }
    let mut ids = HashSet::new();
    let mut positions = HashSet::new();
    let mut bytes = 0_i64;
    for tab in tabs {
        if !ids.insert(tab.id.as_str()) || !positions.insert(tab.position) {
            return Err(AppError::InvalidRequest(
                "tab projection contains duplicate identity or position".into(),
            ));
        }
        bytes = bytes.saturating_add(tab_projection_bytes(tab));
    }
    if bytes > MAX_RESTORED_TAB_BYTES {
        return Err(AppError::ResourceLimit(
            "tab projection exceeds its metadata byte limit".into(),
        ));
    }
    Ok(())
}

fn admit_tab_updated_at(value: Option<i64>) -> Option<i64> {
    value.filter(|stamp| *stamp > 0)
}

pub fn tab_projection_bytes(tab: &WorkbenchTab) -> i64 {
    let resource_bytes = tab.resource.as_ref().map_or(0, |resource| {
        resource
            .root_id
            .len()
            .saturating_add(resource.relative_path.len())
    });
    let repository_bytes = tab.repository.as_ref().map_or(0, |repository| {
        repository
            .root_id
            .len()
            .saturating_add(repository.relative_path.len())
    });
    tab.id
        .len()
        .saturating_add(tab.kind.len())
        .saturating_add(tab.title.len())
        .saturating_add(tab.status.as_deref().map_or(0, str::len))
        .saturating_add(resource_bytes)
        .saturating_add(repository_bytes) as i64
}

fn validate_resource_metadata(root: &RootRecord, resource: &ResourceRef) -> AppResult<()> {
    validate_contract_path(&resource.relative_path, false)?;
    let Ok(workspace) = Workspace::open(&root.project_id, &root.id, &root.absolute_path) else {
        return Ok(());
    };
    if workspace
        .root()
        .join(&resource.relative_path)
        .try_exists()?
    {
        workspace.validate_resource(resource, false)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use rusqlite::params;
    use uuid::Uuid;

    use super::*;
    use crate::persistence::Database;

    struct Fixture {
        folder: std::path::PathBuf,
        database: Database,
        project_id: String,
        root_id: String,
    }

    impl Fixture {
        fn new() -> Self {
            let folder = std::env::temp_dir().join(format!("sage-restore-{}", Uuid::new_v4()));
            fs::create_dir_all(&folder).unwrap();
            fs::write(folder.join("main.rs"), "fn main() {}\n").unwrap();
            let database = Database::open(Path::new(":memory:")).unwrap();
            let project = database.create_project_from_root(&folder).unwrap();
            Self {
                folder,
                database,
                project_id: project.id,
                root_id: project.roots[0].id.clone(),
            }
        }

        fn insert_tab(&self, id: &str, kind: &str, metadata: &str) {
            self.database
                .connection()
                .execute(
                    "INSERT INTO workbench_tabs
                     (id, project_id, kind, title, position, pinned, dirty, status,
                      metadata_json, created_at, updated_at)
                     VALUES (?1, ?2, ?3, 'corrupt', 1, 0, 0, NULL, ?4, 1, 1)",
                    params![id, self.project_id, kind, metadata],
                )
                .unwrap();
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.folder);
        }
    }

    #[test]
    fn foreign_root_and_malformed_file_metadata_fail_before_bootstrap_ingress() {
        let foreign = Fixture::new();
        let fixture = Fixture::new();
        fixture.insert_tab(
            "foreign-file",
            "file",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}}}}"#,
                foreign.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());

        fixture
            .database
            .connection()
            .execute("DELETE FROM workbench_tabs WHERE id = 'foreign-file'", [])
            .unwrap();
        fixture.insert_tab(
            "malformed-file",
            "file",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"src/../main.rs"}}}}"#,
                fixture.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
    }

    #[test]
    fn unsupported_tab_kind_fails_with_a_typed_restore_error() {
        let fixture = Fixture::new();
        fixture
            .database
            .connection()
            .pragma_update(None, "ignore_check_constraints", true)
            .unwrap();
        fixture.insert_tab("unknown", "review", "{}");
        fixture
            .database
            .connection()
            .pragma_update(None, "ignore_check_constraints", false)
            .unwrap();
        assert!(fixture.database.bootstrap_snapshot().is_err());
    }

    #[test]
    fn corrupt_selected_root_and_resource_repair_to_the_first_root() {
        let fixture = Fixture::new();
        fixture
            .database
            .connection()
            .execute(
                "UPDATE project_layouts
                 SET selected_root_id = 'foreign', selected_relative_path = 'src/../main.rs'
                 WHERE project_id = ?1",
                [&fixture.project_id],
            )
            .unwrap();
        let snapshot = fixture.database.bootstrap_snapshot().unwrap();
        assert_eq!(
            snapshot.selected_root_id.as_deref(),
            Some(fixture.root_id.as_str())
        );
        assert!(snapshot.selected_resource.is_none());
    }

    #[test]
    fn oversized_layout_selection_fails_before_string_materialization() {
        let fixture = Fixture::new();
        fixture
            .database
            .connection()
            .execute(
                "UPDATE project_layouts
                 SET selected_relative_path = ?2 WHERE project_id = ?1",
                params![
                    fixture.project_id,
                    "x".repeat(MAX_LAYOUT_SELECTION_BYTES as usize + 1)
                ],
            )
            .unwrap();
        assert!(matches!(
            fixture.database.bootstrap_snapshot(),
            Err(AppError::ResourceLimit(_))
        ));
    }

    #[cfg(windows)]
    #[test]
    fn layout_persistence_rejects_existing_windows_case_aliases() {
        let fixture = Fixture::new();
        let mut snapshot = fixture.database.bootstrap_snapshot().unwrap();
        let resource = ResourceRef {
            root_id: fixture.root_id.clone(),
            relative_path: "MAIN.RS".into(),
        };
        snapshot.tabs.push(WorkbenchTab {
            id: "case-alias".into(),
            kind: "file".into(),
            title: "MAIN.RS".into(),
            position: 1,
            pinned: false,
            dirty: false,
            status: None,
            updated_at: None,
            resource: Some(resource.clone()),
            repository: None,
        });
        assert!(
            fixture
                .database
                .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                    project_id: fixture.project_id.clone(),
                    tabs: snapshot.tabs,
                    active_tab_id: snapshot.active_tab_id,
                    selected_root_id: Some(fixture.root_id.clone()),
                    selected_resource: Some(resource),
                })
                .is_err()
        );
    }

    #[test]
    fn restored_diff_tabs_retain_nested_repository_identity() {
        let fixture = Fixture::new();
        let mut snapshot = fixture.database.bootstrap_snapshot().unwrap();
        snapshot.tabs.push(WorkbenchTab {
            id: "diff:nested".into(),
            kind: "diff".into(),
            title: "main.rs".into(),
            position: 1,
            pinned: false,
            dirty: false,
            status: None,
            updated_at: None,
            resource: Some(ResourceRef {
                root_id: fixture.root_id.clone(),
                relative_path: "main.rs".into(),
            }),
            repository: Some(RepositoryRef {
                root_id: fixture.root_id.clone(),
                relative_path: "vendor/nested".into(),
            }),
        });
        fixture
            .database
            .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                project_id: fixture.project_id.clone(),
                tabs: snapshot.tabs,
                active_tab_id: Some("diff:nested".into()),
                selected_root_id: Some(fixture.root_id.clone()),
                selected_resource: snapshot.selected_resource,
            })
            .unwrap();
        let restored = fixture.database.bootstrap_snapshot().unwrap();
        let diff = restored
            .tabs
            .iter()
            .find(|tab| tab.id == "diff:nested")
            .expect("nested Diff tab");
        assert_eq!(
            diff.repository
                .as_ref()
                .map(|repository| repository.relative_path.as_str()),
            Some("vendor/nested")
        );
        assert_eq!(
            diff.resource
                .as_ref()
                .map(|resource| resource.relative_path.as_str()),
            Some("main.rs")
        );
    }

    fn persist_extra_tab(fixture: &Fixture, tab: WorkbenchTab) -> AppResult<()> {
        let mut snapshot = fixture.database.bootstrap_snapshot().unwrap();
        snapshot.tabs.push(tab);
        fixture
            .database
            .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                project_id: fixture.project_id.clone(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: Some(fixture.root_id.clone()),
                selected_resource: snapshot.selected_resource,
            })
    }

    fn nested_diff_tab(
        id: &str,
        resource_root: &str,
        repository_root: &str,
        kind: &str,
    ) -> WorkbenchTab {
        WorkbenchTab {
            id: id.into(),
            kind: kind.into(),
            title: "main.rs".into(),
            position: 1,
            pinned: false,
            dirty: false,
            status: None,
            updated_at: None,
            resource: Some(ResourceRef {
                root_id: resource_root.into(),
                relative_path: "main.rs".into(),
            }),
            repository: Some(RepositoryRef {
                root_id: repository_root.into(),
                relative_path: "vendor/nested".into(),
            }),
        }
    }

    #[test]
    fn durable_repository_ref_is_ownership_checked() {
        let foreign = Fixture::new();
        let fixture = Fixture::new();

        assert!(
            persist_extra_tab(
                &fixture,
                nested_diff_tab("file-with-repo", &fixture.root_id, &fixture.root_id, "file",),
            )
            .is_err()
        );
        assert!(
            persist_extra_tab(
                &fixture,
                nested_diff_tab(
                    "diff-mismatch",
                    &fixture.root_id,
                    "not-the-resource-root",
                    "diff",
                ),
            )
            .is_err()
        );
        assert!(
            persist_extra_tab(
                &fixture,
                nested_diff_tab("diff-foreign", &fixture.root_id, &foreign.root_id, "diff",),
            )
            .is_err()
        );

        fixture.insert_tab(
            "restored-file-repo",
            "file",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"{}","relativePath":"vendor/nested"}}}}"#,
                fixture.root_id, fixture.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
        fixture
            .database
            .connection()
            .execute(
                "DELETE FROM workbench_tabs WHERE id = 'restored-file-repo'",
                [],
            )
            .unwrap();

        fixture.insert_tab(
            "restored-mismatch",
            "diff",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"not-the-resource-root","relativePath":"vendor/nested"}}}}"#,
                fixture.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
        fixture
            .database
            .connection()
            .execute(
                "DELETE FROM workbench_tabs WHERE id = 'restored-mismatch'",
                [],
            )
            .unwrap();

        fixture.insert_tab(
            "restored-foreign-repo",
            "diff",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"{}","relativePath":"vendor/nested"}}}}"#,
                fixture.root_id, foreign.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
    }

    #[test]
    fn empty_path_repository_ref_is_rejected_not_dropped() {
        let foreign = Fixture::new();
        let fixture = Fixture::new();

        assert!(
            persist_extra_tab(
                &fixture,
                WorkbenchTab {
                    repository: Some(RepositoryRef {
                        root_id: fixture.root_id.clone(),
                        relative_path: String::new(),
                    }),
                    ..nested_diff_tab(
                        "file-empty-repo",
                        &fixture.root_id,
                        &fixture.root_id,
                        "file",
                    )
                },
            )
            .is_err()
        );
        assert!(
            persist_extra_tab(
                &fixture,
                WorkbenchTab {
                    repository: Some(RepositoryRef {
                        root_id: "not-the-resource-root".into(),
                        relative_path: String::new(),
                    }),
                    ..nested_diff_tab(
                        "diff-empty-mismatch",
                        &fixture.root_id,
                        "not-the-resource-root",
                        "diff",
                    )
                },
            )
            .is_err()
        );
        assert!(
            persist_extra_tab(
                &fixture,
                WorkbenchTab {
                    repository: Some(RepositoryRef {
                        root_id: foreign.root_id.clone(),
                        relative_path: String::new(),
                    }),
                    ..nested_diff_tab(
                        "diff-empty-foreign",
                        &fixture.root_id,
                        &foreign.root_id,
                        "diff",
                    )
                },
            )
            .is_err()
        );

        fixture.insert_tab(
            "restored-empty-file-repo",
            "file",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"{}","relativePath":""}}}}"#,
                fixture.root_id, fixture.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
        fixture
            .database
            .connection()
            .execute(
                "DELETE FROM workbench_tabs WHERE id = 'restored-empty-file-repo'",
                [],
            )
            .unwrap();

        fixture.insert_tab(
            "restored-empty-mismatch",
            "diff",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"not-the-resource-root","relativePath":""}}}}"#,
                fixture.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
        fixture
            .database
            .connection()
            .execute(
                "DELETE FROM workbench_tabs WHERE id = 'restored-empty-mismatch'",
                [],
            )
            .unwrap();

        fixture.insert_tab(
            "restored-empty-foreign",
            "diff",
            &format!(
                r#"{{"resource":{{"rootId":"{}","relativePath":"main.rs"}},"repository":{{"rootId":"{}","relativePath":""}}}}"#,
                fixture.root_id, foreign.root_id
            ),
        );
        assert!(fixture.database.bootstrap_snapshot().is_err());
        fixture
            .database
            .connection()
            .execute(
                "DELETE FROM workbench_tabs WHERE id = 'restored-empty-foreign'",
                [],
            )
            .unwrap();

        let mut snapshot = fixture.database.bootstrap_snapshot().unwrap();
        snapshot.tabs.push(WorkbenchTab {
            repository: Some(RepositoryRef {
                root_id: fixture.root_id.clone(),
                relative_path: String::new(),
            }),
            ..nested_diff_tab(
                "diff-empty-root",
                &fixture.root_id,
                &fixture.root_id,
                "diff",
            )
        });
        fixture
            .database
            .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                project_id: fixture.project_id.clone(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: Some(fixture.root_id.clone()),
                selected_resource: snapshot.selected_resource,
            })
            .unwrap();
        let restored = fixture.database.bootstrap_snapshot().unwrap();
        let diff = restored
            .tabs
            .iter()
            .find(|tab| tab.id == "diff-empty-root")
            .expect("root-repo Diff tab");
        assert_eq!(
            diff.repository
                .as_ref()
                .map(|repository| repository.relative_path.as_str()),
            Some("")
        );
    }

    #[test]
    fn restored_chat_tabs_project_durable_chat_updated_at() {
        let fixture = Fixture::new();
        let snapshot = fixture.database.bootstrap_snapshot().unwrap();
        let chat = snapshot
            .tabs
            .iter()
            .find(|tab| tab.kind == "chat")
            .expect("default chat tab");
        let durable = 1_724_630_400_000_i64;
        fixture
            .database
            .connection()
            .execute(
                "UPDATE chats SET updated_at = ?2 WHERE project_id = ?1 AND id = ?3",
                params![fixture.project_id, durable, chat.id],
            )
            .unwrap();
        fixture
            .database
            .connection()
            .execute(
                "UPDATE workbench_tabs SET updated_at = 99 WHERE id = ?1",
                [&chat.id],
            )
            .unwrap();
        let restored = fixture.database.bootstrap_snapshot().unwrap();
        let restored_chat = restored
            .tabs
            .iter()
            .find(|tab| tab.id == chat.id)
            .expect("restored chat tab");
        assert_eq!(restored_chat.updated_at, Some(durable));
        assert!(
            restored
                .tabs
                .iter()
                .filter(|tab| tab.kind != "chat")
                .all(|tab| tab.updated_at != Some(durable))
        );
    }
}
