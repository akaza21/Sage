use crate::project_registry::ProjectRegistryStore;
use crate::project_registry::create::ProjectCreateStore;
use std::{fs, path::Path, sync::Arc};

use uuid::Uuid;

use super::*;
use sage_domain::{file_operations::FileCreationOutcome, resource::ResourceRef};

fn fixture() -> (
    std::path::PathBuf,
    Arc<AppState>,
    sage_domain::project::ProjectSummary,
) {
    let root = std::env::temp_dir().join(format!("sage-create-command-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("src")).unwrap();
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    (root, Arc::new(AppState::new(database)), project)
}

fn request(project_id: &str, root_id: &str, basename: &str) -> CreateProjectFileRequest {
    CreateProjectFileRequest {
        project_id: project_id.into(),
        project_generation: 7,
        expected_transition_epoch: 0,
        parent: ResourceRef {
            root_id: root_id.into(),
            relative_path: "src".into(),
        },
        basename: basename.into(),
    }
}

#[test]
fn returns_exact_identity_and_authoritative_parent_listing() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root, state, project) = fixture();
        let result = execute(&state, request(&project.id, &project.roots[0].id, "new.rs"))
            .await
            .unwrap();
        assert_eq!(result.outcome, FileCreationOutcome::Committed);
        assert!(result.resource_confirmed);
        assert!(result.warning.is_none());
        assert_eq!(result.project_id, project.id);
        assert_eq!(result.project_generation, 7);
        assert_eq!(result.transition_epoch, 0);
        assert_eq!(result.resource.relative_path, "src/new.rs");
        assert!(result.parent_listing.entries.iter().any(|entry| {
            entry.resource == result.resource && entry.kind == "file" && entry.byte_len == Some(0)
        }));
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn creates_folder_with_exact_identity_and_authoritative_parent_listing() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root, state, project) = fixture();
        let result = execute_folder(&state, request(&project.id, &project.roots[0].id, "nested"))
            .await
            .unwrap();
        assert_eq!(result.outcome, FileCreationOutcome::Committed);
        assert!(result.resource_confirmed);
        #[cfg(windows)]
        assert!(result.warning.as_deref().unwrap().contains("durability"));
        #[cfg(not(windows))]
        assert!(result.warning.is_none());
        assert_eq!(result.resource.relative_path, "src/nested");
        assert!(result.parent_listing.entries.iter().any(|entry| {
            entry.resource == result.resource
                && entry.kind == "directory"
                && entry.byte_len.is_none()
        }));
        assert!(root.join("src/nested").is_dir());
        fs::remove_dir_all(root).unwrap();
    });
}

#[test]
fn bounds_project_and_root_ids_before_database_access() {
    let exact = "a".repeat(512);
    let plus_one = "a".repeat(513);
    assert!(validate_request_id("Project ID", &exact).is_ok());
    assert!(validate_request_id("root ID", &exact).is_ok());
    assert!(validate_request_id("Project ID", &plus_one).is_err());
    assert!(validate_request_id("root ID", &plus_one).is_err());
    assert!(validate_request_id("Project ID", "").is_err());
}

#[test]
fn interrupted_committed_operation_recovers_as_effect_unknown() {
    let (root, state, project) = fixture();
    fs::write(root.join("src/interrupted.rs"), b"").unwrap();
    let result = recover_effect_unknown(
        state.database.clone(),
        request(&project.id, &project.roots[0].id, "interrupted.rs"),
        0,
        ResourceKind::File,
    )
    .unwrap();
    assert_eq!(result.outcome, FileCreationOutcome::EffectUnknown);
    assert!(result.resource_confirmed);
    assert!(result.warning.as_deref().unwrap().contains("unknown"));
    assert!(
        result
            .parent_listing
            .entries
            .iter()
            .any(|entry| entry.resource == result.resource && entry.kind == "file")
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn interrupted_folder_creation_recovers_as_effect_unknown() {
    let (root, state, project) = fixture();
    fs::create_dir(root.join("src/interrupted")).unwrap();
    let result = recover_effect_unknown(
        state.database.clone(),
        request(&project.id, &project.roots[0].id, "interrupted"),
        0,
        ResourceKind::Folder,
    )
    .unwrap();
    assert_eq!(result.outcome, FileCreationOutcome::EffectUnknown);
    assert!(result.resource_confirmed);
    assert!(result.warning.as_deref().unwrap().contains("unknown"));
    assert!(
        result
            .parent_listing
            .entries
            .iter()
            .any(|entry| { entry.resource == result.resource && entry.kind == "directory" })
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn same_relative_parent_in_two_roots_is_root_qualified() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root_a, state, project) = fixture();
        let root_b = root_a.with_extension("second");
        fs::create_dir_all(root_b.join("src")).unwrap();
        let project = state
            .database
            .add_project_root(&project.id, &root_b)
            .unwrap();
        let result = execute(
            &state,
            request(&project.id, &project.roots[1].id, "only-b.rs"),
        )
        .await
        .unwrap();
        assert_eq!(result.resource.root_id, project.roots[1].id);
        assert!(!root_a.join("src/only-b.rs").exists());
        assert!(root_b.join("src/only-b.rs").exists());
        let folder = execute_folder(&state, request(&project.id, &project.roots[1].id, "only-b"))
            .await
            .unwrap();
        assert_eq!(folder.resource.root_id, project.roots[1].id);
        assert!(!root_a.join("src/only-b").exists());
        assert!(root_b.join("src/only-b").is_dir());
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    });
}

#[test]
fn stale_epoch_and_project_switch_fail_before_mutation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root_a, state, project_a) = fixture();
        *state.project_transitions.lock().await = 1;
        let stale = execute(
            &state,
            request(&project_a.id, &project_a.roots[0].id, "stale.rs"),
        )
        .await
        .unwrap_err();
        assert!(stale.to_string().contains("project changed"));
        assert!(!root_a.join("src/stale.rs").exists());

        *state.project_transitions.lock().await = 0;
        let root_b = root_a.with_extension("project-b");
        fs::create_dir_all(&root_b).unwrap();
        state.database.create_project_from_root(&root_b).unwrap();
        let switched = execute(
            &state,
            request(&project_a.id, &project_a.roots[0].id, "switched.rs"),
        )
        .await
        .unwrap_err();
        assert!(switched.to_string().contains("not active"));
        assert!(!root_a.join("src/switched.rs").exists());
        fs::remove_dir_all(root_a).unwrap();
        fs::remove_dir_all(root_b).unwrap();
    });
}

#[test]
fn unavailable_root_and_native_collision_preserve_existing_content() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root, state, project) = fixture();
        fs::write(root.join("src/existing.rs"), "preserve\n").unwrap();
        assert!(
            execute(
                &state,
                request(&project.id, &project.roots[0].id, "existing.rs")
            )
            .await
            .is_err()
        );
        assert_eq!(
            fs::read_to_string(root.join("src/existing.rs")).unwrap(),
            "preserve\n"
        );
        fs::remove_dir_all(&root).unwrap();
        assert!(
            execute(
                &state,
                request(&project.id, &project.roots[0].id, "offline.rs")
            )
            .await
            .is_err()
        );
    });
}

#[test]
fn full_explorer_directory_is_rejected_before_creation() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let (root, state, project) = fixture();
        for index in 0..sage_workspace::MAX_DIRECTORY_ENTRIES {
            fs::write(root.join("src").join(format!("{index:03}.txt")), "").unwrap();
        }
        let error = execute(
            &state,
            request(&project.id, &project.roots[0].id, "overflow.rs"),
        )
        .await
        .unwrap_err();
        assert!(error.to_string().contains("500-entry"));
        assert!(!root.join("src/overflow.rs").exists());
        fs::remove_dir_all(root).unwrap();
    });
}
