use super::*;
use crate::chat_archive::{ChatArchiveStore, delete::ArchivedChatDeleteStore};
use crate::chat_move::ChatMoveStore;
use crate::project_registry::{ProjectRegistryStore, create::ProjectCreateStore};
use crate::workbench_persistence::store::WorkbenchStore;
use sage_domain::chat::{ChatDeleteRequest, ChatLifecycleRequest};

pub(super) fn layout(database: &Database) -> ProjectLayoutRequest {
    let snapshot = database.bootstrap_snapshot().unwrap();
    ProjectLayoutRequest {
        project_id: snapshot.project.unwrap().id,
        tabs: snapshot.tabs,
        active_tab_id: snapshot.active_tab_id,
        selected_root_id: snapshot.selected_root_id,
        selected_resource: snapshot.selected_resource,
    }
}

#[test]
fn private_roots_are_plain_absolute_paths_including_previously_persisted_roots() {
    let directory = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-root-path-{}", Uuid::new_v4()));
    fs::create_dir_all(&directory).unwrap();
    let database_path = directory.join("sage.db");
    let database = crate::database::open_database(&database_path).unwrap();
    create(&database, None, &Uuid::new_v4().to_string()).unwrap();
    let project = database.bootstrap_snapshot().unwrap().project.unwrap();
    let expected = directory.join("chat-workspaces").join(&project.id);
    let root = database.root_records(&project.id).unwrap().remove(0);
    assert!(root.absolute_path.is_absolute());
    assert!(!root.absolute_path.to_string_lossy().starts_with(r"\\?\"));
    assert_eq!(root.absolute_path, expected);
    assert!(expected.is_dir());
    // Reproduce the spelling saved by the private-Chat feature before this fix.
    let legacy = expected.canonicalize().unwrap();
    let legacy_key = if cfg!(windows) {
        legacy.to_string_lossy().to_lowercase()
    } else {
        legacy.to_string_lossy().into_owned()
    };
    database
        .connection()
        .execute(
            "UPDATE project_roots SET absolute_path=?1,canonical_key=?2 WHERE id=?3",
            params![legacy.to_str().unwrap(), legacy_key, root.id],
        )
        .unwrap();
    drop(database);
    let database = crate::database::open_database(&database_path).unwrap();
    assert_eq!(
        database.root_records(&project.id).unwrap()[0].absolute_path,
        expected
    );
    assert_eq!(
        database.root_record(&root.id).unwrap().absolute_path,
        expected
    );
    drop(database);
    fs::remove_dir_all(directory).unwrap();
}

#[test]
fn selecting_a_project_refuses_private_files_and_preserves_the_chat_until_explicit_deletion() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-reparent-files-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let real = database
        .create_project_from_root(&root.join("user"))
        .unwrap();
    let chat = Uuid::new_v4().to_string();
    create(&database, None, &chat).unwrap();
    let current = layout(&database);
    let folder = database.root_records(&current.project_id).unwrap()[0]
        .absolute_path
        .clone();
    fs::write(folder.join("notes.txt"), "Keep this saved work").unwrap();
    let result = set_project(&database, &current, &chat, Some(&real.id));
    assert!(result.is_err(), "reparenting must not discard saved files");
    assert!(
        result
            .unwrap_err()
            .public_message()
            .contains("This chat's folder has files. Start a new chat in the project instead.")
    );
    let moved = database.move_chat(&MoveChatRequest {
        project_id: current.project_id.clone(),
        chat_id: chat.clone(),
        target_project_id: real.id.clone(),
        current_layout: current.clone(),
    });
    assert!(
        moved
            .unwrap_err()
            .public_message()
            .contains("This chat's folder has files. Start a new chat in the project instead.")
    );
    assert_eq!(
        fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "Keep this saved work"
    );
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.project.unwrap().id, current.project_id);
    assert_eq!(snapshot.active_tab_id.as_deref(), Some(chat.as_str()));
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: current.project_id.clone(),
            chat_id: chat.clone(),
            current_layout: current.clone(),
        })
        .unwrap();
    database
        .delete_archived_chat(&ChatDeleteRequest {
            project_id: current.project_id,
            chat_id: chat,
        })
        .unwrap();
    assert!(
        !folder.exists(),
        "explicit chat deletion reclaims its files"
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_chat_owns_a_folder_moves_only_without_sessions_and_cleans_up() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-chat-{}", Uuid::new_v4()));
    fs::create_dir_all(root.join("user")).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let real = database
        .create_project_from_root(&root.join("user"))
        .unwrap();
    let before = layout(&database);
    let chat = Uuid::new_v4().to_string();
    create(&database, Some(&before), &chat).unwrap();
    let private = database.bootstrap_snapshot().unwrap();
    let project = private.project.unwrap();
    assert!(project.is_internal);
    assert_eq!(private.active_tab_id.as_deref(), Some(chat.as_str()));
    assert_eq!(private.projects.len(), 1);
    assert_eq!(private.unscoped_chats.len(), 1);
    let folder = database.root_records(&project.id).unwrap()[0]
        .absolute_path
        .clone();
    assert!(
        std::path::Path::new(&crate::project_registry::conservative_overlap_key(&folder))
            .starts_with(crate::project_registry::conservative_overlap_key(
                &root.canonicalize().unwrap().join("chat-workspaces"),
            ))
    );
    assert!(folder.is_dir());
    set_project(&database, &layout(&database), &chat, Some(&real.id)).unwrap();
    assert_eq!(
        database.bootstrap_snapshot().unwrap().project.unwrap().id,
        real.id
    );
    assert!(!folder.exists());
    set_project(&database, &layout(&database), &chat, None).unwrap();
    let scope = layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    // A first Turn freezes ownership.
    use crate::session_events::SessionEventStore;
    database
        .append_simulator_turn(&chat, "hello", "world")
        .unwrap();
    assert!(set_project(&database, &layout(&database), &chat, Some(&real.id)).is_err());
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: scope.project_id.clone(),
            chat_id: chat.clone(),
            current_layout: layout(&database),
        })
        .unwrap();
    database
        .delete_archived_chat(&ChatDeleteRequest {
            project_id: scope.project_id,
            chat_id: chat,
        })
        .unwrap();
    assert!(!folder.exists());
    assert!(
        database
            .bootstrap_snapshot()
            .unwrap()
            .unscoped_chats
            .is_empty()
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn existing_database_migrates_without_changing_projects_chats_or_history() {
    use crate::session_events::SessionEventStore;
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-migration-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let path = root.join("sage.db");
    let database = crate::database::open_database(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let chat = before.active_tab_id.unwrap();
    database
        .append_simulator_turn(&chat, "Keep this question", "Keep this answer")
        .unwrap();
    crate::acp::migration::test_schema::rollback_to_version(&database.connection(), 31).unwrap();
    drop(database);
    let migrated = crate::database::open_database(&path).unwrap();
    let after = migrated.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.unwrap().id, project.id);
    assert_eq!(after.projects.len(), 1);
    assert_eq!(after.active_tab_id.as_deref(), Some(chat.as_str()));
    assert_eq!(after.messages[0].text, "Keep this question");
    assert_eq!(after.messages[1].text, "Keep this answer");
    assert!(after.unscoped_chats.is_empty());
    drop(migrated);
    let reopened = crate::database::open_database(&path).unwrap();
    assert_eq!(
        reopened.bootstrap_snapshot().unwrap().messages[0].text,
        "Keep this question"
    );
    drop(reopened);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn cleanup_refuses_deep_trees_and_retains_a_retry_receipt() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-cleanup-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    let chat = Uuid::new_v4().to_string();
    create(&database, None, &chat).unwrap();
    let scope = layout(&database);
    let folder = database.root_records(&scope.project_id).unwrap()[0]
        .absolute_path
        .clone();
    let mut deep = folder.clone();
    for _ in 0..65 {
        deep = deep.join("d");
    }
    fs::create_dir_all(&deep).unwrap();
    fs::write(deep.join("keep"), "not deleted past the bound").unwrap();
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: scope.project_id.clone(),
            chat_id: chat.clone(),
            current_layout: scope.clone(),
        })
        .unwrap();
    database
        .delete_archived_chat(&ChatDeleteRequest {
            project_id: scope.project_id.clone(),
            chat_id: chat,
        })
        .unwrap();
    assert!(deep.join("keep").exists());
    assert!(
        database
            .project_summary(&scope.project_id)
            .unwrap()
            .is_internal
    );
    assert!(database.project_list().unwrap().is_empty());
    // Remove the over-depth subtree as the owner would, then retry cleanup.
    fs::remove_dir_all(folder.join("d")).unwrap();
    cleanup::empty_projects(&database).unwrap();
    assert!(!folder.exists());
    assert!(database.project_summary(&scope.project_id).is_err());
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn hidden_project_ids_cannot_be_selected_as_user_projects() {
    use crate::chat_move::ChatMoveStore;
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-target-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    create(&database, None, &Uuid::new_v4().to_string()).unwrap();
    let first = layout(&database).project_id;
    let chat = Uuid::new_v4().to_string();
    create(&database, None, &chat).unwrap();
    let before = layout(&database);
    assert!(set_project(&database, &before, &chat, Some(&first)).is_err());
    assert!(
        database
            .move_chat(&MoveChatRequest {
                project_id: before.project_id.clone(),
                chat_id: chat,
                target_project_id: first,
                current_layout: before.clone()
            })
            .is_err()
    );
    assert_eq!(layout(&database).project_id, before.project_id);
    assert_eq!(
        database.bootstrap_snapshot().unwrap().unscoped_chats.len(),
        2
    );
    drop(database);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn private_chat_capacity_never_silently_hides_older_chats() {
    let root = std::env::current_dir()
        .unwrap()
        .join("work")
        .join(format!("private-capacity-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = crate::database::open_database(&root.join("sage.db")).unwrap();
    database.create_project_from_root(&root).unwrap();
    let real = layout(&database);
    for _ in 0..MAX_UNSCOPED_CHATS {
        create(&database, None, &Uuid::new_v4().to_string()).unwrap();
    }
    let before = layout(&database);
    let refused = create(&database, None, &Uuid::new_v4().to_string()).unwrap_err();
    assert!(
        refused
            .public_message()
            .contains("64 chats without a project")
    );
    assert_eq!(list(&database).unwrap().len(), 64);
    assert_eq!(layout(&database).active_tab_id, before.active_tab_id);
    assert!(
        set_project(
            &database,
            &real,
            real.active_tab_id.as_deref().unwrap(),
            None
        )
        .unwrap_err()
        .public_message()
        .contains("64 chats without a project")
    );
    assert_eq!(list(&database).unwrap().len(), 64);
    assert_eq!(layout(&database).active_tab_id, before.active_tab_id);
    drop(database);
    fs::remove_dir_all(root).unwrap();
}
