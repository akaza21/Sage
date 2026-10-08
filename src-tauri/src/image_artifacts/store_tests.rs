use std::io::Cursor;

use image::{DynamicImage, ImageFormat, Rgba, RgbaImage};

use super::*;

fn fixture() -> (Connection, std::path::PathBuf, Vec<u8>) {
    let connection = migration::open_or_migrate(Path::new(":memory:")).unwrap();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('chat','project',0,'Chat',1,1)", []).unwrap();
    let (connection, _) = connection.into_parts();
    let root = std::env::temp_dir()
        .join(format!("sage-image-test-{}", Uuid::new_v4()))
        .join("image-artifacts");
    let mut output = Cursor::new(Vec::new());
    DynamicImage::ImageRgba8(RgbaImage::from_pixel(64, 32, Rgba([20, 40, 60, 128])))
        .write_to(&mut output, ImageFormat::Png)
        .unwrap();
    (connection, root, output.into_inner())
}

#[test]
fn owner_bound_pending_artifact_round_trips_a_verified_preview() {
    let (mut connection, root, encoded) = fixture();
    let attachment = create_pending_image(
        &mut connection,
        &root,
        "project",
        "chat",
        &encoded,
        "image.png",
    )
    .unwrap();
    assert_eq!(attachment.width, 64);
    assert_eq!(attachment.height, 32);
    assert!(
        load_image_preview(
            &connection,
            &root,
            "project",
            "chat",
            &attachment.artifact_id
        )
        .unwrap()
        .is_some()
    );
    assert!(
        load_image_preview(&connection, &root, "other", "chat", &attachment.artifact_id)
            .unwrap()
            .is_none()
    );
    fs::write(thumb_path(&root, &attachment.artifact_id), b"tampered").unwrap();
    assert!(
        load_image_preview(
            &connection,
            &root,
            "project",
            "chat",
            &attachment.artifact_id
        )
        .is_err()
    );
    fs::remove_file(thumb_path(&root, &attachment.artifact_id)).unwrap();
    fs::remove_file(full_path(&root, &attachment.artifact_id)).unwrap();
    fs::remove_dir(&root).unwrap();
    fs::remove_dir(root.parent().unwrap()).unwrap();
}

#[test]
fn unavailable_owner_leaves_no_live_artifact() {
    let (mut connection, root, encoded) = fixture();
    assert!(
        create_pending_image(
            &mut connection,
            &root,
            "project",
            "missing",
            &encoded,
            "image.png"
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
    fs::remove_dir(&root).unwrap();
    fs::remove_dir(root.parent().unwrap()).unwrap();
}

#[test]
fn discarded_pending_image_releases_both_files_without_touching_another_owner() {
    let (mut connection, root, encoded) = fixture();
    let image = create_pending_image(
        &mut connection,
        &root,
        "project",
        "chat",
        &encoded,
        "image.png",
    )
    .unwrap();
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
    fs::remove_dir(&root).unwrap();
    fs::remove_dir(root.parent().unwrap()).unwrap();
}

#[test]
fn quota_evicts_oldest_retired_thumbnail_before_rejecting_active_refs() {
    let (mut connection, _, _) = fixture();
    let older = Uuid::new_v4().to_string();
    let newer = Uuid::new_v4().to_string();
    for (id, created) in [(&older, 1_i64), (&newer, 2)] {
        connection.execute(
            "INSERT INTO chat_image_artifacts(id,project_id,chat_id,lifecycle,mime_type,width,height,full_bytes,full_sha256,thumbnail_bytes,thumbnail_sha256,label,created_at,updated_at)
             VALUES (?1,'project','chat','retired','image/png',1,1,1,?2,10,?2,'image',?3,?3)",
            params![id, "a".repeat(64), created],
        ).unwrap();
    }
    let tx = connection.transaction().unwrap();
    let evicted = admit_quota(&tx, "project", "chat", 15, 30, 30).unwrap();
    assert_eq!(evicted, vec![older]);
    assert_eq!(
        tx.query_row("SELECT COUNT(*) FROM chat_image_artifacts", [], |row| row
            .get::<_, i64>(
            0
        ))
        .unwrap(),
        1
    );
    tx.commit().unwrap();
}
