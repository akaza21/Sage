use rusqlite::Connection;
use sage_core::error::AppError;
use uuid::Uuid;

use super::*;
use crate::editor_persistence::store::EditorBufferStore;
use crate::{
    editor_persistence::{MAX_TOTAL_DIRTY_BYTES, MAX_TOTAL_DIRTY_METADATA_BYTES},
    project_registry::MAX_PROJECTS,
};

#[cfg(not(windows))]
fn complete_legacy_upgrade_for_runtime(path: &Path) {
    let mut connection = Connection::open(path).unwrap();
    crate::project_migration::migrate(&mut connection).unwrap();
    crate::acp::migration::complete_v4_upgrade_for_test(&mut connection).unwrap();
    connection.close().unwrap();
}

fn legacy_fixture() -> (std::path::PathBuf, std::path::PathBuf) {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/main.rs"}"#);
    (folder, path)
}

fn migration_applied(path: &Path) -> bool {
    Connection::open(path)
        .unwrap()
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn fill_project_registry(connection: &Connection) {
    connection
        .execute(
            "WITH RECURSIVE n(value) AS (
               SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value < ?1
             )
             INSERT INTO projects
             SELECT printf('extra-%d', value), 'extra', 1, 1, 1 FROM n",
            [MAX_PROJECTS - 1],
        )
        .unwrap();
}

#[test]
fn limit_plus_one_project_sentinel_rolls_back_before_collection() {
    let (folder, path) = legacy_fixture();
    let connection = Connection::open(&path).unwrap();
    for index in 1..=MAX_PROJECTS {
        connection
            .execute(
                "INSERT INTO projects VALUES (?1, 'extra', 1, 1, 1)",
                [format!("extra-{index}")],
            )
            .unwrap();
    }
    drop(connection);

    assert!(matches!(
        crate::database::open_database(&path),
        Err(AppError::ResourceLimit(_))
    ));
    assert!(!migration_applied(&path));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn limit_plus_one_root_sentinel_rolls_back_before_collection() {
    let (folder, path) = legacy_fixture();
    let connection = Connection::open(&path).unwrap();
    fill_project_registry(&connection);
    connection
        .execute(
            "WITH RECURSIVE n(value) AS (
               SELECT 1 UNION ALL SELECT value + 1 FROM n WHERE value <= 7
             )
             INSERT INTO project_roots
             SELECT 'legacy', printf('C:/legacy-root-%d', value), value FROM n",
            [],
        )
        .unwrap();
    connection
        .execute(
            "WITH RECURSIVE
               p(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM p WHERE value < ?1
               ),
               r(value) AS (
                 SELECT 0 UNION ALL SELECT value + 1 FROM r WHERE value < 7
               )
             INSERT INTO project_roots
             SELECT printf('extra-%d', p.value),
                    printf('C:/root-%d-%d', p.value, r.value), r.value
             FROM p CROSS JOIN r",
            [MAX_PROJECTS - 1],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO project_roots VALUES ('legacy', 'C:/overflow', 9)",
            [],
        )
        .unwrap();
    drop(connection);

    #[cfg(windows)]
    let result = crate::database::open_database(&path).map(|_| ());
    #[cfg(not(windows))]
    let result = {
        let mut connection = Connection::open(&path).unwrap();
        let result = crate::project_migration::migrate(&mut connection);
        connection.close().unwrap();
        result
    };
    assert!(matches!(result, Err(AppError::ResourceLimit(_))));
    assert!(!migration_applied(&path));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn limit_plus_one_tab_sentinel_rolls_back_before_collection() {
    let (folder, path) = legacy_fixture();
    let connection = Connection::open(&path).unwrap();
    fill_project_registry(&connection);
    connection
        .execute(
            "WITH RECURSIVE n(value) AS (
               SELECT 2 UNION ALL SELECT value + 1 FROM n WHERE value < 63
             )
             INSERT INTO workbench_tabs
             SELECT printf('legacy-tab-%d', value), 'legacy', 'chat', 'chat',
                    value, 0, 0, NULL, '{}', 1, 1 FROM n",
            [],
        )
        .unwrap();
    connection
        .execute(
            "WITH RECURSIVE
               p(value) AS (
                 SELECT 1 UNION ALL SELECT value + 1 FROM p WHERE value < ?1
               ),
               t(value) AS (
                 SELECT 0 UNION ALL SELECT value + 1 FROM t WHERE value < 63
               )
             INSERT INTO workbench_tabs
             SELECT printf('extra-tab-%d-%d', p.value, t.value),
                    printf('extra-%d', p.value), 'chat', 'chat', t.value,
                    0, 0, NULL, '{}', 1, 1 FROM p CROSS JOIN t",
            [MAX_PROJECTS - 1],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workbench_tabs
             VALUES ('overflow-tab', 'legacy', 'chat', 'chat', 64,
                     0, 0, NULL, '{}', 1, 1)",
            [],
        )
        .unwrap();
    drop(connection);

    assert!(matches!(
        crate::database::open_database(&path),
        Err(AppError::ResourceLimit(_))
    ));
    assert!(!migration_applied(&path));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn omitted_dirty_metadata_bytes_are_bounded_and_roll_back() {
    let (folder, path) = legacy_fixture();
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE editor_buffers SET pending_temp_path = ?1",
            ["x".repeat(MAX_TOTAL_DIRTY_METADATA_BYTES as usize + 1)],
        )
        .unwrap();

    #[cfg(windows)]
    let result = crate::database::open_database(&path).map(|_| ());
    #[cfg(not(windows))]
    let result = {
        let mut connection = Connection::open(&path).unwrap();
        let result = crate::project_migration::migrate(&mut connection);
        connection.close().unwrap();
        result
    };
    assert!(matches!(result, Err(AppError::ResourceLimit(_))));
    assert!(!migration_applied(&path));
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn exact_legacy_content_limit_allows_ordinary_metadata() {
    let (folder, path) = legacy_fixture();
    Connection::open(&path)
        .unwrap()
        .execute(
            "UPDATE editor_buffers
             SET buffer_content = ?1, buffer_byte_len = ?2",
            rusqlite::params![
                "x".repeat(MAX_TOTAL_DIRTY_BYTES as usize),
                MAX_TOTAL_DIRTY_BYTES
            ],
        )
        .unwrap();

    #[cfg(not(windows))]
    complete_legacy_upgrade_for_runtime(&path);
    let database = crate::database::open_database(&path).unwrap();
    assert_eq!(
        database.dirty_editor_buffers("legacy", None).unwrap().len(),
        1
    );
    drop(database);
    fs::remove_dir_all(folder).unwrap();
}
