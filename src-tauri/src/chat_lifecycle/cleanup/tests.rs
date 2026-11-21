use super::*;
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};
use crate::workbench_persistence::store::WorkbenchStore;
use rusqlite::params;
use std::{fs, path::Path};

fn project(database: &Database, count: usize) -> (String, Vec<String>) {
    let active = database.active_project_id().unwrap();
    let project = database
        .create_named_project_if_active("Legacy", &[], active.as_deref())
        .unwrap();
    let mut connection = database.write_connection().unwrap();
    let transaction = connection.transaction().unwrap();
    let chats = (0..count)
        .map(|_| super::super::insert_new_chat(&transaction, &project.id, true, None).unwrap())
        .collect();
    transaction.commit().unwrap();
    (project.id, chats)
}

#[test]
fn startup_deletes_unselected_empty_chats_and_keeps_the_selected_chat() {
    let root = std::env::temp_dir().join(format!("sage-legacy-cleanup-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("sage.db");
    let database = crate::database::open_database(&path).unwrap();
    let (_, chats) = project(&database, 2);
    drop(database);
    let database = crate::database::open_database(&path).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        snapshot.tabs.iter().map(|tab| &tab.id).collect::<Vec<_>>(),
        vec![&chats[1]]
    );
    assert_eq!(snapshot.active_tab_id.as_ref(), Some(&chats[1]));
    assert_eq!(legacy_empty_chats(&database).unwrap(), 0);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_keeps_user_intent_history_archives_and_saved_input() {
    use crate::session_events::SessionEventStore;
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let (project, chats) = project(&database, 9);
    {
        let connection = database.connection();
        connection
            .execute(
                "UPDATE chats SET title_source='user',title='Renamed' WHERE id=?1",
                [&chats[1]],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE chats SET title_source='fork' WHERE id=?1",
                [&chats[2]],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE workbench_tabs SET pinned=1 WHERE id=?1",
                [&chats[3]],
            )
            .unwrap();
        connection
            .execute("DELETE FROM workbench_tabs WHERE id=?1", [&chats[4]])
            .unwrap();
        connection
            .execute(
                "INSERT INTO chat_queued_prompts VALUES (?1,?2,?3,1)",
                params![
                    project,
                    chats[5],
                    r#"[{"id":"queued","text":"Send later"}]"#
                ],
            )
            .unwrap();
        connection
            .execute(
                "UPDATE workbench_tabs SET metadata_json=?2 WHERE id=?1",
                params![chats[6], r#"{"composerText":"Unsent"}"#],
            )
            .unwrap();
    }
    database
        .append_simulator_turn(&chats[7], "Keep this prompt", "Answer")
        .unwrap();
    // Prove history protects independently of the generated title.
    database
        .connection()
        .execute(
            "UPDATE chats SET title_source='default' WHERE id=?1",
            [&chats[7]],
        )
        .unwrap();
    assert_eq!(legacy_empty_chats(&database).unwrap(), 1);
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.tabs.len(), 7);
    assert_eq!(snapshot.archived_chats[0].id, chats[4]);
    for id in chats.iter().skip(1).filter(|id| *id != &chats[4]) {
        assert!(snapshot.tabs.iter().any(|tab| &tab.id == id));
    }
    assert_eq!(legacy_empty_chats(&database).unwrap(), 0);
}

#[test]
fn cleanup_is_bounded_and_repeated_runs_finish_the_remaining_chats() {
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    for _ in 0..9 {
        project(&database, 61);
    }
    assert_eq!(legacy_empty_chats(&database).unwrap(), 512);
    assert_eq!(legacy_empty_chats(&database).unwrap(), 28);
    assert_eq!(legacy_empty_chats(&database).unwrap(), 0);
    let projects = database.project_list().unwrap();
    assert_eq!(projects.len(), 9);
    assert!(projects.iter().all(|project| project.rail_chats.len() == 1));
}

#[test]
fn cleanup_preserves_private_user_files_and_reclaims_an_empty_private_folder() {
    let root = std::env::temp_dir().join(format!("sage-private-legacy-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let mut folders = Vec::new();
    for has_files in [true, false] {
        let chat = uuid::Uuid::new_v4().to_string();
        crate::private_chat::create(&database, None, &chat).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.unwrap().id;
        let folder = database.root_records(&project).unwrap()[0]
            .absolute_path
            .clone();
        if has_files {
            fs::write(folder.join("user.txt"), "Keep my work").unwrap();
        }
        database
            .connection()
            .execute(
                "UPDATE project_layouts SET active_tab_id=NULL WHERE project_id=?1",
                [&project],
            )
            .unwrap();
        folders.push(folder);
    }
    assert_eq!(legacy_empty_chats(&database).unwrap(), 1);
    assert_eq!(
        fs::read_to_string(folders[0].join("user.txt")).unwrap(),
        "Keep my work"
    );
    assert!(!folders[1].exists());
    assert_eq!(
        database.bootstrap_snapshot().unwrap().unscoped_chats.len(),
        1
    );
    assert_eq!(legacy_empty_chats(&database).unwrap(), 0);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_preserves_the_historical_welcome_transcript() {
    let database = crate::database::open_database(Path::new(":memory:")).unwrap();
    let (project, chats) = project(&database, 2);
    {
        let mut connection = database.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        let created: i64 = transaction
            .query_row(
                "SELECT created_at FROM chats WHERE id=?1",
                [&chats[0]],
                |row| row.get(0),
            )
            .unwrap();
        crate::session_events::seed_legacy_welcome(
            &transaction,
            &project,
            &chats[0],
            &uuid::Uuid::new_v4().to_string(),
            created,
            r#"{"role":"assistant","text":"Welcome"}"#,
            "simulatorTurnCommitted",
        )
        .unwrap();
        transaction.commit().unwrap();
    }
    assert!(
        crate::session_events::legacy_welcome::turn_id(&database.connection(), &chats[0])
            .unwrap()
            .is_some()
    );
    assert_eq!(legacy_empty_chats(&database).unwrap(), 0);
    assert_eq!(database.bootstrap_snapshot().unwrap().tabs.len(), 2);
}

#[test]
fn private_reclamation_preserves_files_arriving_after_admission() {
    use crate::chat_purge::ChatPurgeStore;
    let root = std::env::temp_dir().join(format!("sage-private-arrival-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let chat = uuid::Uuid::new_v4().to_string();
    crate::private_chat::create(&database, None, &chat).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project = snapshot.project.unwrap().id;
    let folder = database.root_records(&project).unwrap()[0]
        .absolute_path
        .clone();
    let result = database.purge_chat_with_finalizer(
        &project,
        &chat,
        |connection| {
            crate::private_chat::require_empty_folder(&folder)?;
            if !connection.is_autocommit() {
                fs::write(
                    folder.join("arrived.txt"),
                    "User work arriving during deletion",
                )?;
            }
            Ok(())
        },
        |transaction| crate::private_chat::reclaim_empty_in(transaction, &project, &folder),
    );
    assert!(result.is_err());
    assert_eq!(
        fs::read_to_string(folder.join("arrived.txt")).unwrap(),
        "User work arriving during deletion"
    );
    assert_eq!(
        database.bootstrap_snapshot().unwrap().active_tab_id,
        Some(chat)
    );
    drop(database);
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    assert_eq!(
        database.bootstrap_snapshot().unwrap().unscoped_chats.len(),
        1
    );
    assert!(folder.join("arrived.txt").exists());
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_reclamation_retries_a_missing_folder_without_a_recursive_receipt() {
    let root = std::env::temp_dir().join(format!("sage-private-retry-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let chat = uuid::Uuid::new_v4().to_string();
    crate::private_chat::create(&database, None, &chat).unwrap();
    let project = database.bootstrap_snapshot().unwrap().project.unwrap().id;
    let folder = database.root_records(&project).unwrap()[0]
        .absolute_path
        .clone();
    database
        .connection()
        .execute(
            "UPDATE project_layouts SET active_tab_id=NULL WHERE project_id=?1",
            [&project],
        )
        .unwrap();
    // A crash after empty-directory removal, before the purge commits.
    fs::remove_dir(&folder).unwrap();
    assert_eq!(legacy_empty_chats(&database).unwrap(), 1);
    fs::create_dir(&folder).unwrap();
    fs::write(
        folder.join("arrived-later.txt"),
        "Never authorized for deletion",
    )
    .unwrap();
    drop(database);
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    assert!(
        database
            .bootstrap_snapshot()
            .unwrap()
            .unscoped_chats
            .is_empty()
    );
    assert_eq!(
        fs::read_to_string(folder.join("arrived-later.txt")).unwrap(),
        "Never authorized for deletion"
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}
