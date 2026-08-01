use std::{fs, path::Path};

use rusqlite::{Connection, params};
use uuid::Uuid;

use crate::persistence::Database;

const INITIAL: &str = include_str!("../../migrations/001_initial.sql");
const DIFF_TABS: &str = include_str!("../../migrations/002_diff_tabs.sql");
const EDITOR_BUFFERS: &str = include_str!("../../migrations/003_editor_buffers.sql");

fn seed_legacy(path: &Path, root: &Path, metadata: &str) {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch(INITIAL).unwrap();
    connection.execute_batch(DIFF_TABS).unwrap();
    connection.execute_batch(EDITOR_BUFFERS).unwrap();
    connection
        .execute(
            "INSERT INTO projects VALUES ('legacy', 'Legacy', 1, 1, 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO project_roots VALUES ('legacy', ?1, 0)",
            [root.to_string_lossy().as_ref()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workbench_tabs
             VALUES ('chat', 'legacy', 'chat', 'Chat', 0, 1, 0, 'idle',
                     '{}', 1, 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO workbench_tabs
             VALUES ('file', 'legacy', 'file', 'main.rs', 1, 0, 1, NULL,
                     ?1, 1, 1)",
            [metadata],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions
             (id, project_id, chat_tab_id, harness_id, model_id, status,
              created_at, updated_at)
             VALUES ('session', 'legacy', 'chat', 'sage-simulator',
                     'sim-fast', 'idle', 1, 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_events
             VALUES ('event', 'session', 1, 'message',
                     '{\"role\":\"assistant\",\"text\":\"preserved\"}', 1)",
            [],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO editor_buffers
             VALUES ('legacy', 'src/main.rs', 'base', 4, 1, 'lf',
                     'dirty', 5, 'buffer', 2, NULL, 2)",
            [],
        )
        .unwrap();
}

#[test]
fn migrates_legacy_root_tabs_session_and_dirty_buffer() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/main.rs"}"#);

    let database = Database::open(&path).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project = snapshot.project.unwrap();
    assert_eq!(project.id, "legacy");
    assert_eq!(project.roots.len(), 1);
    let file = snapshot.tabs.iter().find(|tab| tab.id == "file").unwrap();
    assert_eq!(file.resource.as_ref().unwrap().relative_path, "src/main.rs");
    assert_eq!(snapshot.messages[0].text, "preserved");
    let buffers = database.dirty_editor_buffers("legacy", None).unwrap();
    assert_eq!(buffers[0].content, "dirty");
    assert_eq!(buffers[0].resource.root_id, project.roots[0].id);
    let version: i64 = database
        .connection()
        .query_row("SELECT MAX(version) FROM schema_migrations", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(version, 4);
    drop(database);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn missing_legacy_root_and_dirty_buffer_survive_upgrade_and_reopen() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("missing-root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/main.rs"}"#);
    fs::remove_dir_all(&root).unwrap();

    let database = Database::open(&path).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        snapshot.project.as_ref().unwrap().roots[0].availability,
        "missing"
    );
    assert_eq!(
        database.dirty_editor_buffers("legacy", None).unwrap()[0].content,
        "dirty"
    );
    drop(database);

    let reopened = Database::open(&path).unwrap();
    assert_eq!(
        reopened
            .bootstrap_snapshot()
            .unwrap()
            .project
            .unwrap()
            .roots[0]
            .availability,
        "missing"
    );
    assert_eq!(
        reopened.dirty_editor_buffers("legacy", None).unwrap().len(),
        1
    );
    drop(reopened);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn failed_migration_rolls_back_without_destroying_legacy_rows() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, "not-json");

    assert!(Database::open(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    let migrated: bool = connection
        .query_row(
            "SELECT EXISTS(
               SELECT 1 FROM schema_migrations WHERE version = 4
             )",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_buffer: String = connection
        .query_row(
            "SELECT buffer_content FROM editor_buffers
             WHERE project_id = 'legacy' AND relative_path = 'src/main.rs'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let legacy_root: String = connection
        .query_row(
            "SELECT absolute_path FROM project_roots WHERE project_id = ?1",
            params!["legacy"],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!migrated);
    assert_eq!(legacy_buffer, "dirty");
    assert_eq!(legacy_root, root.to_string_lossy());
    drop(connection);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn over_limit_legacy_buffers_fail_before_collection_and_roll_back() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/main.rs"}"#);
    let connection = Connection::open(&path).unwrap();
    for index in 0..16 {
        connection
            .execute(
                "INSERT INTO editor_buffers
                 VALUES ('legacy', ?1, 'base', 1, NULL, 'lf', 'x', 1,
                         'buffer', 1, NULL, 1)",
                [format!("extra-{index}.txt")],
            )
            .unwrap();
    }
    drop(connection);

    assert!(Database::open(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM editor_buffers", [], |row| row.get(0))
        .unwrap();
    let migrated: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, 17);
    assert!(!migrated);
    drop(connection);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn over_limit_actual_legacy_buffer_bytes_fail_and_roll_back() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/main.rs"}"#);
    let connection = Connection::open(&path).unwrap();
    connection
        .execute(
            "UPDATE editor_buffers
             SET buffer_content = ?1, buffer_byte_len = 1
             WHERE project_id = 'legacy'",
            ["x".repeat(8 * 1024 * 1024 + 1)],
        )
        .unwrap();
    drop(connection);

    assert!(Database::open(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    let migrated: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let declared: i64 = connection
        .query_row("SELECT buffer_byte_len FROM editor_buffers", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert!(!migrated);
    assert_eq!(declared, 1);
    drop(connection);
    fs::remove_dir_all(folder).unwrap();
}

#[test]
fn noncanonical_legacy_resource_alias_rolls_back() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"src/./main.rs"}"#);
    assert!(Database::open(&path).is_err());
    let connection = Connection::open(&path).unwrap();
    let migrated: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = 4)",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(!migrated);
    drop(connection);
    fs::remove_dir_all(folder).unwrap();
}

#[cfg(windows)]
#[test]
fn windows_migration_recovers_actual_root_relative_casing() {
    let folder = std::env::temp_dir().join(format!("sage-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&folder).unwrap();
    let root = folder.join("root");
    fs::create_dir_all(root.join("src")).unwrap();
    fs::write(root.join("src/main.rs"), "fn main() {}\n").unwrap();
    let path = folder.join("sage.db");
    seed_legacy(&path, &root, r#"{"resourcePath":"SRC/MAIN.RS"}"#);

    let database = Database::open(&path).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let file = snapshot.tabs.iter().find(|tab| tab.id == "file").unwrap();
    assert_eq!(file.resource.as_ref().unwrap().relative_path, "src/main.rs");
    drop(database);
    fs::remove_dir_all(folder).unwrap();
}

mod limits;
