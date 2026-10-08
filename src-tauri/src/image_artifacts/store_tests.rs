use super::*;

fn fixture() -> (Connection, std::path::PathBuf) {
    let opened = migration::open_or_migrate(Path::new(":memory:")).unwrap();
    let (connection, _) = opened.into_parts();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1)", []).unwrap();
    let root = std::env::temp_dir()
        .join(format!("sage-image-test-{}", Uuid::new_v4()))
        .join("image-artifacts");
    (connection, root)
}

fn cleanup(root: &Path) {
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir(root.parent().unwrap()).unwrap();
}

#[test]
fn owner_bound_preview_is_hash_verified() {
    let (connection, root) = fixture();
    let image = crate::image_artifacts::insert_test_image(
        &connection,
        &root,
        "project",
        "chat",
        "image.png",
    );
    assert!(
        load_image_preview(&connection, &root, "project", "chat", &image.artifact_id)
            .unwrap()
            .is_some()
    );
    assert!(
        load_image_preview(&connection, &root, "other", "chat", &image.artifact_id)
            .unwrap()
            .is_none()
    );
    fs::write(thumb_path(&root, &image.artifact_id), b"tampered").unwrap();
    assert!(load_image_preview(&connection, &root, "project", "chat", &image.artifact_id).is_err());
    cleanup(&root);
}

#[test]
fn pending_discard_releases_both_files_without_touching_another_owner() {
    let (mut connection, root) = fixture();
    let image = crate::image_artifacts::insert_test_image(
        &connection,
        &root,
        "project",
        "chat",
        "image.png",
    );
    assert!(
        !discard_pending_image(&mut connection, &root, "other", "chat", &image.artifact_id)
            .unwrap()
    );
    assert!(full_path(&root, &image.artifact_id).exists());
    assert!(
        discard_pending_image(
            &mut connection,
            &root,
            "project",
            "chat",
            &image.artifact_id
        )
        .unwrap()
    );
    assert!(!full_path(&root, &image.artifact_id).exists());
    assert!(!thumb_path(&root, &image.artifact_id).exists());
    cleanup(&root);
}
