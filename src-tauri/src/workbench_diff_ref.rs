use sage_core::error::{AppError, AppResult};

use crate::{
    acp_protocol::file_diff::DiffRef,
    domain::{ResourceRef, WorkbenchTab},
};

const FORBIDDEN_SNAPSHOT_KEYS: &[&str] = &[
    "oldText",
    "newText",
    "old_text",
    "new_text",
    "oldBytes",
    "newBytes",
    "old_blob",
    "new_blob",
    "oldBlob",
    "newBlob",
    "contentDigest",
    "content_digest",
    "snapshot",
    "snapshotRevision",
    "snapshot_revision",
    "pairState",
    "pair_available",
    "resourcePath",
];

pub fn diff_ref_projection_bytes(reference: &DiffRef) -> i64 {
    (reference.project_id.len()
        + reference.chat_id.len()
        + reference.session_id.len()
        + reference.turn_id.len()
        + reference.item_id.len()
        + reference.root_id.len()
        + reference.relative_path.len()
        + reference.content_digest.len()
        + reference.runtime_epoch.to_string().len()
        + reference.snapshot_revision.to_string().len()
        + 8) as i64
}

fn invalid(message: impl Into<String>) -> AppError {
    AppError::InvalidRequest(message.into())
}

fn validate_owned_ref(project_id: &str, reference: &DiffRef) -> AppResult<()> {
    reference.validate().map_err(|error| {
        invalid(format!(
            "persisted snapshot reference is {}: {}",
            match error {
                crate::acp_protocol::file_diff::SnapshotReadError::Unavailable => "unavailable",
                crate::acp_protocol::file_diff::SnapshotReadError::Stale => "stale",
                crate::acp_protocol::file_diff::SnapshotReadError::Corrupt => "corrupt",
                crate::acp_protocol::file_diff::SnapshotReadError::Forbidden => "forbidden",
            },
            error.message()
        ))
    })?;
    if reference.project_id != project_id {
        return Err(invalid(
            "persisted snapshot reference project does not match its tab project",
        ));
    }
    Ok(())
}

pub fn validate_diff_ref_for_persist(project_id: &str, tab: &WorkbenchTab) -> AppResult<()> {
    let Some(reference) = tab.diff_ref.as_ref() else {
        return Ok(());
    };
    if tab.kind != "diff" {
        return Err(invalid("only Diff tabs may carry a snapshot reference"));
    }
    if tab.repository.is_some() {
        return Err(invalid("snapshot Diff tabs require no Git repository"));
    }
    let Some(resource) = tab.resource.as_ref() else {
        return Err(invalid("snapshot Diff tabs require a resource identity"));
    };
    if reference.root_id != resource.root_id || reference.relative_path != resource.relative_path {
        return Err(invalid(
            "persisted snapshot reference does not match its tab resource",
        ));
    }
    validate_owned_ref(project_id, reference)
}

pub fn parse_diff_ref_from_metadata(
    project_id: &str,
    kind: &str,
    resource: Option<&ResourceRef>,
    repository: Option<&crate::domain::RepositoryRef>,
    object: &serde_json::Map<String, serde_json::Value>,
) -> AppResult<Option<DiffRef>> {
    for key in FORBIDDEN_SNAPSHOT_KEYS {
        if object.contains_key(*key) {
            return Err(invalid("persisted tab contains raw snapshot text metadata"));
        }
    }
    let Some(raw) = object.get("diffRef") else {
        return Ok(None);
    };
    let reference: DiffRef =
        serde_json::from_value(raw.clone()).map_err(|error| invalid(error.to_string()))?;
    if kind != "diff" {
        return Err(invalid("only Diff tabs may carry a snapshot reference"));
    }
    if repository.is_some() {
        return Err(invalid("snapshot Diff tabs require no Git repository"));
    }
    let Some(resource) = resource else {
        return Err(invalid("snapshot Diff tabs require a resource identity"));
    };
    if reference.root_id != resource.root_id || reference.relative_path != resource.relative_path {
        return Err(invalid(
            "persisted snapshot reference does not match its tab resource",
        ));
    }
    validate_owned_ref(project_id, &reference)?;
    Ok(Some(reference))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::{RepositoryRef, ResourceRef, WorkbenchTab};
    use std::{fs, path::Path};
    use uuid::Uuid;

    fn valid_ref(project_id: &str, root_id: &str) -> DiffRef {
        serde_json::from_value(serde_json::json!({
            "projectId": project_id,
            "chatId": "chat-one",
            "sessionId": "session-one",
            "runtimeEpoch": "1",
            "turnId": "turn-one",
            "itemId": "item-one",
            "diffOrdinal": 0,
            "snapshotRevision": "1",
            "rootId": root_id,
            "relativePath": "src/a.ts",
            "contentDigest": "a".repeat(64),
        }))
        .unwrap()
    }

    fn snapshot_tab(project_id: &str, root_id: &str) -> WorkbenchTab {
        WorkbenchTab {
            id: "diff:snapshot:one".into(),
            kind: "diff".into(),
            title: "a.ts".into(),
            position: 1,
            pinned: false,
            dirty: false,
            status: None,
            updated_at: None,
            finished_at: None,
            resource: Some(ResourceRef {
                root_id: root_id.into(),
                relative_path: "src/a.ts".into(),
            }),
            repository: None,
            diff_ref: Some(valid_ref(project_id, root_id)),
        }
    }

    #[test]
    fn valid_snapshot_tab_passes_persist_validation() {
        let tab = snapshot_tab("project-one", "root-one");
        assert!(validate_diff_ref_for_persist("project-one", &tab).is_ok());
    }

    #[test]
    fn diff_ref_is_rejected_on_wrong_kind_repository_or_resource() {
        let root = "root-one";
        let project = "project-one";
        let mut tab = snapshot_tab(project, root);
        tab.kind = "file".into();
        assert!(validate_diff_ref_for_persist(project, &tab).is_err());
        tab.kind = "diff".into();
        tab.repository = Some(RepositoryRef {
            root_id: root.into(),
            relative_path: String::new(),
        });
        assert!(validate_diff_ref_for_persist(project, &tab).is_err());
        tab.repository = None;
        tab.resource = Some(ResourceRef {
            root_id: root.into(),
            relative_path: "src/other.ts".into(),
        });
        assert!(validate_diff_ref_for_persist(project, &tab).is_err());
    }

    #[test]
    fn foreign_project_owner_is_rejected() {
        let tab = snapshot_tab("project-one", "root-one");
        assert!(validate_diff_ref_for_persist("project-two", &tab).is_err());
    }

    #[test]
    fn malformed_counters_and_digest_are_rejected() {
        for (field, value) in [
            ("runtimeEpoch", serde_json::json!("0")),
            ("runtimeEpoch", serde_json::json!("01")),
            ("snapshotRevision", serde_json::json!("0")),
            ("snapshotRevision", serde_json::json!("9223372036854775808")),
            ("contentDigest", serde_json::json!("A".repeat(64))),
            ("contentDigest", serde_json::json!("short")),
        ] {
            let mut raw = serde_json::json!({
                "projectId": "project-one",
                "chatId": "chat-one",
                "sessionId": "session-one",
                "runtimeEpoch": "1",
                "turnId": "turn-one",
                "itemId": "item-one",
                "diffOrdinal": 0,
                "snapshotRevision": "1",
                "rootId": "root-one",
                "relativePath": "src/a.ts",
                "contentDigest": "a".repeat(64),
            });
            raw[field] = value;
            let parsed: Result<DiffRef, _> = serde_json::from_value(raw);
            assert!(
                parsed.is_err() || {
                    let reference = parsed.unwrap();
                    validate_owned_ref("project-one", &reference).is_err()
                }
            );
        }
    }

    #[test]
    fn raw_snapshot_keys_are_rejected_on_restore() {
        let resource = ResourceRef {
            root_id: "root-one".into(),
            relative_path: "src/a.ts".into(),
        };
        let mut object = serde_json::Map::new();
        object.insert("resource".into(), serde_json::to_value(&resource).unwrap());
        object.insert(
            "diffRef".into(),
            serde_json::to_value(valid_ref("project-one", "root-one")).unwrap(),
        );
        object.insert("oldText".into(), serde_json::json!("stolen"));
        assert!(
            parse_diff_ref_from_metadata("project-one", "diff", Some(&resource), None, &object)
                .is_err()
        );
    }

    #[test]
    fn projection_bytes_include_the_reference() {
        let tab = snapshot_tab("project-one", "root-one");
        let without = WorkbenchTab {
            diff_ref: None,
            ..tab.clone()
        };
        let expected = crate::workbench_restore::tab_projection_bytes(&without)
            + diff_ref_projection_bytes(tab.diff_ref.as_ref().unwrap());
        assert_eq!(
            crate::workbench_restore::tab_projection_bytes(&tab),
            expected
        );
    }

    #[test]
    fn snapshot_tab_round_trips_ref_only_without_text_or_git_fallback() {
        let folder = std::env::temp_dir().join(format!("sage-diffref-{}", Uuid::new_v4()));
        fs::create_dir_all(&folder).unwrap();
        fs::write(folder.join("src-a.ts"), "old\n").unwrap();
        let database = crate::persistence::Database::open(Path::new(":memory:")).unwrap();
        let project = database.create_project_from_root(&folder).unwrap();
        let root_id = project.roots[0].id.clone();
        let mut snapshot = database.bootstrap_snapshot().unwrap();
        let mut tab = snapshot_tab(&project.id, &root_id);
        tab.position = snapshot.tabs.len() as i64 + 1;
        tab.id = "diff:snapshot:roundtrip".into();
        snapshot.tabs.push(tab.clone());
        database
            .persist_project_layout(&crate::domain::ProjectLayoutRequest {
                project_id: project.id.clone(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: Some(root_id.clone()),
                selected_resource: snapshot.selected_resource,
            })
            .unwrap();
        let metadata: String = database
            .connection()
            .query_row(
                "SELECT metadata_json FROM workbench_tabs WHERE id='diff:snapshot:roundtrip'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert!(metadata.contains("diffRef"));
        assert!(!metadata.contains("oldText"));
        assert!(!metadata.contains("newText"));
        let restored = database.bootstrap_snapshot().unwrap();
        let found = restored
            .tabs
            .iter()
            .find(|candidate| candidate.id == "diff:snapshot:roundtrip")
            .expect("snapshot tab survives restart");
        assert_eq!(found.diff_ref, tab.diff_ref);
        assert_eq!(found.resource.as_ref().unwrap().relative_path, "src/a.ts");
        assert!(found.repository.is_none());
        fs::remove_dir_all(&folder).unwrap();
    }
}
