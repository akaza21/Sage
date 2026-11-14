use super::*;
use crate::chat_lifecycle::store::ChatLifecycleStore;
use crate::chat_purge::HISTORY_GUARDS;
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;

struct Fixture {
    root: std::path::PathBuf,
    database: Database,
    project: String,
    side: String,
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("sage-side-purge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project = snapshot.project.as_ref().unwrap().id.clone();
    let parent = snapshot.active_tab_id.clone().unwrap();
    let layout = sage_domain::chat::ProjectLayoutRequest {
        project_id: project.clone(),
        tabs: snapshot.tabs,
        active_tab_id: Some(parent.clone()),
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    };
    let side = database
        .create_chat_with_parent(&project, &layout, Some(&parent))
        .unwrap();
    Fixture {
        root,
        database,
        project,
        side,
    }
}

fn side_chats(database: &Database) -> i64 {
    database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM chats WHERE id IN (SELECT id FROM workbench_tabs WHERE json_extract(metadata_json,'$.ephemeral')=1)",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn a_missing_history_guard_is_named_and_refuses_before_deleting_anything() {
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    let restore: String = database
        .connection()
        .query_row(
            "SELECT sql FROM sqlite_schema WHERE name='deny_fact_delete'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    database
        .connection()
        .execute_batch("DROP TRIGGER deny_fact_delete")
        .unwrap();
    let error = database.remove_side_chat(&project, &side).unwrap_err();
    assert!(
        error.to_string().contains("deny_fact_delete"),
        "the refusal names the missing guard: {error}"
    );
    assert_eq!(side_chats(&database), 1);
    database.connection().execute_batch(&restore).unwrap();
    database.remove_side_chat(&project, &side).unwrap();
    assert_eq!(side_chats(&database), 0);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn a_failed_purge_warns_and_never_blocks_startup_or_the_other_purges() {
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    let other_root = root.join("other-project");
    std::fs::create_dir_all(&other_root).unwrap();
    let other = database.create_project_from_root(&other_root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let parent = snapshot.active_tab_id.clone().unwrap();
    let layout = sage_domain::chat::ProjectLayoutRequest {
        project_id: other.id.clone(),
        tabs: snapshot.tabs,
        active_tab_id: Some(parent.clone()),
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    };
    let other_side = database
        .create_chat_with_parent(&other.id, &layout, Some(&parent))
        .unwrap();
    // Fault-inject a ledger that cannot charge this Project's purge. Its CHECK
    // constraint rolls that one transaction back; another Project can converge.
    database
        .connection()
        .execute(
            "UPDATE acp_project_usage SET chat_count=0 WHERE project_id=?1",
            [&project],
        )
        .unwrap();
    assert!(database.purge_side_chats().is_err());
    assert!(is_temporary(&database.connection(), &side).unwrap());
    assert!(!is_temporary(&database.connection(), &other_side).unwrap());
    assert_eq!(side_chats(&database), 1);
    drop(database);
    let reopened =
        crate::database::open_database(&root.join("sage.db")).expect("startup must not block");
    assert_eq!(side_chats(&reopened), 1);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn image_rows_leave_in_bounded_batches_and_files_only_after_commit() {
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    let artifact_root = database.image_artifact_root().unwrap().to_path_buf();
    let images = (0..5)
        .map(|index| {
            crate::image_artifacts::insert_test_image(
                &database.connection(),
                &artifact_root,
                &project,
                &side,
                &format!("side-{index}.png"),
            )
            .artifact_id
        })
        .collect::<Vec<_>>();
    {
        // Batches run inside the caller's transaction; a rollback keeps every
        // row and file.
        let mut connection = database.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        let taken = crate::chat_purge::take_chat_images(&transaction, &project, &side, 2).unwrap();
        assert_eq!(taken.len(), 5);
    }
    let rows: i64 = database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM chat_image_artifacts WHERE chat_id=?1",
            [&side],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(rows, 5);
    for id in &images {
        assert!(artifact_root.join(format!("{id}.full")).is_file());
    }
    database.remove_side_chat(&project, &side).unwrap();
    for id in &images {
        assert!(!artifact_root.join(format!("{id}.full")).exists());
        assert!(!artifact_root.join(format!("{id}.thumb")).exists());
    }
    assert_eq!(side_chats(&database), 0);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

/// A purge that would leave a dangling reference is refused by SQLite at
/// COMMIT (deferred foreign keys), surfacing as
/// `AppError::Database(SqliteFailure { extended_code: 787 /* FOREIGNKEY */ })`.
/// The whole transaction rolls back, so the Chat and every lifted guard
/// trigger survive.
#[test]
fn a_purge_leaving_a_dangling_reference_is_refused_at_commit_and_guards_survive() {
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    // A deferrable (NO ACTION) reference the purge does not delete.
    database
        .connection()
        .execute_batch(
            "CREATE TABLE purge_probe(project_id TEXT NOT NULL, chat_id TEXT NOT NULL,
               FOREIGN KEY (project_id, chat_id) REFERENCES chats(project_id, id))",
        )
        .unwrap();
    database
        .connection()
        .execute("INSERT INTO purge_probe VALUES (?1, ?2)", [&project, &side])
        .unwrap();
    let error = database.remove_side_chat(&project, &side).unwrap_err();
    assert!(
        matches!(
            &error,
            AppError::Database(sqlite)
                if matches!(
                    sqlite.downcast_ref::<rusqlite::Error>(),
                    Some(rusqlite::Error::SqliteFailure(failure, _))
                        if failure.extended_code == 787
                )
        ),
        "refused at COMMIT by the deferred foreign-key check: {error:?}"
    );
    assert_eq!(side_chats(&database), 1);
    let guards: i64 = database
        .connection()
        .query_row(
            &format!(
                "SELECT COUNT(*) FROM sqlite_schema WHERE type='trigger' AND name IN ({})",
                HISTORY_GUARDS
                    .iter()
                    .map(|name| format!("'{name}'"))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(guards, HISTORY_GUARDS.len() as i64);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn temporary_authority_comes_from_durable_metadata_not_chat_id() {
    let fixture = fixture();
    assert!(is_temporary(&fixture.database.connection(), &fixture.side).unwrap());
    let connection = fixture.database.connection();
    connection
        .execute(
            "UPDATE workbench_tabs SET metadata_json='{}' WHERE id=?1",
            [&fixture.side],
        )
        .unwrap();
    assert!(!is_temporary(&connection, &fixture.side).unwrap());
    assert!(refuse_persistence(&connection, &fixture.side).is_ok());
    drop(connection);
    assert!(
        fixture
            .database
            .remove_side_chat(&fixture.project, &fixture.side)
            .is_err()
    );
    drop(fixture.database);
    std::fs::remove_dir_all(fixture.root).unwrap();
}
