//! Startup must survive a damaged image-artifact store: a profile copied
//! without its `image-artifacts` folder, or one where that path is unusable.

use std::path::{Path, PathBuf};

use super::open_database;

fn profile_with_one_image() -> (PathBuf, PathBuf) {
    let directory =
        std::env::temp_dir().join(format!("sage-image-startup-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let database_path = directory.join("sage.db");
    let database = open_database(&database_path).unwrap();
    let root = database.image_artifact_root().unwrap().to_path_buf();
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('p','P',1,1,1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at) VALUES('c','p',0,'C',1,1)",
            [],
        )
        .unwrap();
    let image = sage_images::insert_test_image(&connection, &root, "p", "c", "image.png");
    connection
        .execute(
            "UPDATE chat_image_artifacts SET lifecycle='queued',queue_id='q1' WHERE id=?1",
            [&image.artifact_id],
        )
        .unwrap();
    drop(connection);
    drop(database);
    (directory, root)
}

fn image_rows(path: &Path) -> i64 {
    open_database(path)
        .unwrap()
        .connection()
        .query_row("SELECT COUNT(*) FROM chat_image_artifacts", [], |row| {
            row.get(0)
        })
        .unwrap()
}

#[test]
fn startup_survives_a_profile_without_its_image_artifacts_folder() {
    let (directory, root) = profile_with_one_image();
    std::fs::remove_dir_all(&root).unwrap();

    // The row stays (reads fail closed on it); the folder comes back.
    assert_eq!(image_rows(&directory.join("sage.db")), 1);
    assert!(root.is_dir());
    let _ = std::fs::remove_dir_all(directory);
}

#[test]
fn startup_survives_an_image_artifacts_path_that_is_not_a_folder() {
    let (directory, root) = profile_with_one_image();
    std::fs::remove_dir_all(&root).unwrap();
    std::fs::write(&root, b"not a folder").unwrap();

    assert_eq!(image_rows(&directory.join("sage.db")), 1);
    let _ = std::fs::remove_dir_all(directory);
}
