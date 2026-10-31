use super::*;

struct Fixture {
    root: std::path::PathBuf,
    database: Database,
    project: String,
    side: String,
}

fn fixture() -> Fixture {
    let root = std::env::temp_dir().join(format!("sage-side-purge-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&root).unwrap();
    let database = Database::open(&root.join("sage.db")).unwrap();
    database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project = snapshot.project.as_ref().unwrap().id.clone();
    let parent = snapshot.active_tab_id.clone().unwrap();
    let layout = crate::domain::ProjectLayoutRequest {
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
            "SELECT COUNT(*) FROM chats WHERE id GLOB 'side-*'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn temporary_chat_purge_removes_transition_receipts_and_their_quota_charge() {
    use crate::acp::{
        contract::{ClientIntentId, ProjectionRevision, RequiredText, SageChatId, SageProjectId},
        store::{
            StartSessionCommand,
            session_replace::{ReplaceSessionCommand, replace_session},
        },
    };
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    let before: i64 = database
        .connection()
        .query_row(
            "SELECT shared_payload_bytes FROM acp_store_usage",
            [],
            |row| row.get(0),
        )
        .unwrap();
    replace_session(
        &mut database.connection(),
        ReplaceSessionCommand {
            confirm_unknown_delivery: false,
            start: StartSessionCommand {
                project_id: SageProjectId::parse(project.clone()).unwrap(),
                chat_id: SageChatId::parse(side.clone()).unwrap(),
                expected_projection_revision: ProjectionRevision::ZERO,
                client_intent_id: ClientIntentId::parse("side-transition").unwrap(),
                binding_id: RequiredText::new("binding", "fixture").unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: 1,
            },
            expected_head: None,
        },
    )
    .unwrap();
    database.remove_side_chat(&project, &side).unwrap();
    assert_eq!(side_chats(&database), 0);
    assert_eq!(
        database
            .connection()
            .query_row("SELECT COUNT(*) FROM acp_session_transitions", [], |row| {
                row.get::<_, i64>(0)
            })
            .unwrap(),
        0
    );
    assert_eq!(
        database
            .connection()
            .query_row(
                "SELECT shared_payload_bytes FROM acp_store_usage",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        before
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn deleting_side_chat_invalidates_parent_attention() {
    let Fixture {
        root,
        database,
        project,
        side,
    } = fixture();
    let parent: String = database
        .connection()
        .query_row(
            "SELECT json_extract(metadata_json,'$.parentChatId') FROM workbench_tabs WHERE id=?1",
            [&side],
            |row| row.get(0),
        )
        .unwrap();
    database
        .connection()
        .execute(
            "INSERT INTO chat_attention(chat_id,pending_approvals) VALUES(?1,1)",
            [&side],
        )
        .unwrap();
    let before = crate::chat_attention::store::get(
        &database.connection(),
        &project,
        std::slice::from_ref(&parent),
    )
    .unwrap()
    .remove(0);
    assert_eq!(before.pending_approvals, 1);
    database.remove_side_chat(&project, &side).unwrap();
    let after = crate::chat_attention::store::get(&database.connection(), &project, &[parent])
        .unwrap()
        .remove(0);
    assert_eq!(after.pending_approvals, 0);
    assert!(after.activity_position > before.activity_position);
    assert!(after.revision > before.revision);
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
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
        side,
        ..
    } = fixture();
    // A registry row the purge refuses (not a canonical temporary Chat id).
    let columns = database
        .connection()
        .prepare("SELECT name FROM pragma_table_info('chats') ORDER BY cid")
        .unwrap()
        .query_map([], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let values = columns
        .iter()
        .map(|column| {
            if column == "id" {
                "'side-not-a-uuid'".to_owned()
            } else if column == "ordinal" {
                "(SELECT MAX(ordinal)+1 FROM chats)".to_owned()
            } else {
                format!("\"{column}\"")
            }
        })
        .collect::<Vec<_>>()
        .join(",");
    let names = columns
        .iter()
        .map(|column| format!("\"{column}\""))
        .collect::<Vec<_>>()
        .join(",");
    database
        .connection()
        .execute(
            &format!("INSERT INTO chats({names}) SELECT {values} FROM chats WHERE id=?1"),
            [&side],
        )
        .unwrap();
    assert!(database.purge_side_chats().is_err());
    // The valid temporary Chat was still purged.
    assert_eq!(side_chats(&database), 1);
    drop(database);
    let reopened = Database::open(&root.join("sage.db")).expect("startup must not block");
    assert_eq!(side_chats(&reopened), 1);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn image_cleanup_runs_in_bounded_batches_instead_of_failing_at_a_cap() {
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
    assert_eq!(
        database
            .remove_side_chat_images(&project, &side, 2)
            .unwrap(),
        5
    );
    for id in &images {
        assert!(!artifact_root.join(format!("{id}.full")).exists());
        assert!(!artifact_root.join(format!("{id}.thumb")).exists());
    }
    database.remove_side_chat(&project, &side).unwrap();
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
            AppError::Database(rusqlite::Error::SqliteFailure(failure, _))
                if failure.extended_code == 787
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
