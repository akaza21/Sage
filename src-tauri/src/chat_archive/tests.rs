use std::fs;

use uuid::Uuid;

use super::*;
use crate::{
    domain::{ProjectLayoutRequest, ProjectSwitchRequest, WorkbenchTab},
    error::AppError,
};

fn request(
    project_id: &str,
    chat_id: &str,
    snapshot: &crate::domain::BootstrapSnapshot,
) -> ChatLifecycleRequest {
    ChatLifecycleRequest {
        project_id: project_id.into(),
        chat_id: chat_id.into(),
        current_layout: ProjectLayoutRequest {
            project_id: project_id.into(),
            tabs: snapshot.tabs.clone(),
            active_tab_id: snapshot.active_tab_id.clone(),
            selected_root_id: snapshot.selected_root_id.clone(),
            selected_resource: snapshot.selected_resource.clone(),
        },
    }
}

fn insert_detached_chat(database: &Database, project_id: &str, id: &str, ordinal: i64) {
    database
        .connection()
        .execute(
            "INSERT INTO chats(
               id,project_id,ordinal,title,projection_revision,created_at,updated_at
             ) VALUES (?1,?2,?3,?4,0,2,2)",
            rusqlite::params![id, project_id, ordinal, format!("Chat: {id}")],
        )
        .unwrap();
}

fn assert_lifecycle_state_unchanged(
    before: &crate::domain::BootstrapSnapshot,
    after: &crate::domain::BootstrapSnapshot,
) {
    assert_eq!(after.active_tab_id, before.active_tab_id);
    assert_eq!(after.selected_root_id, before.selected_root_id);
    assert_eq!(
        after
            .selected_resource
            .as_ref()
            .map(|value| (&value.root_id, &value.relative_path)),
        before
            .selected_resource
            .as_ref()
            .map(|value| (&value.root_id, &value.relative_path))
    );
    assert_eq!(
        after
            .tabs
            .iter()
            .map(|tab| (
                &tab.id,
                &tab.kind,
                &tab.title,
                tab.position,
                tab.pinned,
                tab.dirty
            ))
            .collect::<Vec<_>>(),
        before
            .tabs
            .iter()
            .map(|tab| (
                &tab.id,
                &tab.kind,
                &tab.title,
                tab.position,
                tab.pinned,
                tab.dirty
            ))
            .collect::<Vec<_>>()
    );
    assert_eq!(after.archived_chats, before.archived_chats);
}

#[test]
fn archive_and_restore_preserve_history_across_restart() {
    let base = std::env::temp_dir().join(format!("sage-chat-archive-{}", Uuid::new_v4()));
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let database_path = base.join("sage.db");
    let database = Database::open(&database_path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    let second_chat_id = database
        .create_chat(&ProjectLayoutRequest {
            project_id: project.id.clone(),
            tabs: initial.tabs,
            active_tab_id: initial.active_tab_id,
            selected_root_id: initial.selected_root_id,
            selected_resource: initial.selected_resource,
        })
        .unwrap();
    database
        .append_simulator_turn(&first_chat_id, "remember me", "preserved")
        .unwrap();

    let attached = database.bootstrap_snapshot().unwrap();
    let mut select_first = request(&project.id, &first_chat_id, &attached).current_layout;
    select_first.active_tab_id = Some(first_chat_id.clone());
    database.persist_project_layout(&select_first).unwrap();
    let attached = database.bootstrap_snapshot().unwrap();
    let expected_session_id = attached.session_id.clone();
    let expected_messages = attached
        .messages
        .iter()
        .map(|message| (message.sequence, message.role.clone(), message.text.clone()))
        .collect::<Vec<_>>();
    let expected_title = attached
        .tabs
        .iter()
        .find(|tab| tab.id == first_chat_id)
        .map(|tab| tab.title.clone())
        .unwrap();
    database
        .archive_chat(&request(&project.id, &first_chat_id, &attached))
        .unwrap();
    let archived = database.bootstrap_snapshot().unwrap();
    assert_eq!(archived.tabs.len(), 1);
    assert_eq!(
        archived.active_tab_id.as_deref(),
        Some(second_chat_id.as_str())
    );
    assert_eq!(
        archived.archived_chats,
        vec![crate::domain::ArchivedChatSummary {
            id: first_chat_id.clone(),
            // The first prompt auto-titles a default-titled chat.
            title: "Chat: remember me".into(),
        }]
    );
    drop(database);

    let reopened = Database::open(&database_path).unwrap();
    let restarted = reopened.bootstrap_snapshot().unwrap();
    assert_eq!(restarted.archived_chats, archived.archived_chats);
    reopened
        .restore_chat(&request(&project.id, &first_chat_id, &restarted))
        .unwrap();
    drop(reopened);

    let restored = Database::open(&database_path).unwrap();
    let snapshot = restored.bootstrap_snapshot().unwrap();
    assert!(snapshot.archived_chats.is_empty());
    assert_eq!(
        snapshot.active_tab_id.as_deref(),
        Some(first_chat_id.as_str())
    );
    assert_eq!(snapshot.session_id, expected_session_id);
    assert_eq!(
        snapshot
            .messages
            .iter()
            .map(|message| (message.sequence, message.role.clone(), message.text.clone()))
            .collect::<Vec<_>>(),
        expected_messages
    );
    assert_eq!(
        snapshot
            .tabs
            .iter()
            .find(|tab| tab.id == first_chat_id)
            .map(|tab| tab.title.as_str()),
        Some(expected_title.as_str())
    );
    drop(restored);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn active_archive_selects_the_next_chat_then_previous_and_rejects_the_last() {
    let root = std::env::temp_dir().join(format!("sage-chat-archive-order-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    let second_chat_id = database
        .create_chat(&request(&project.id, &first_chat_id, &initial).current_layout)
        .unwrap();
    let second = database.bootstrap_snapshot().unwrap();
    let third_chat_id = database
        .create_chat(&request(&project.id, &second_chat_id, &second).current_layout)
        .unwrap();
    let third = database.bootstrap_snapshot().unwrap();
    let mut select_second = request(&project.id, &second_chat_id, &third).current_layout;
    select_second.active_tab_id = Some(second_chat_id.clone());
    database.persist_project_layout(&select_second).unwrap();

    let active_second = database.bootstrap_snapshot().unwrap();
    database
        .archive_chat(&request(&project.id, &second_chat_id, &active_second))
        .unwrap();
    let active_third = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        active_third.active_tab_id.as_deref(),
        Some(third_chat_id.as_str())
    );

    database
        .archive_chat(&request(&project.id, &third_chat_id, &active_third))
        .unwrap();
    let active_first = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        active_first.active_tab_id.as_deref(),
        Some(first_chat_id.as_str())
    );
    assert_eq!(active_first.archived_chats.len(), 2);

    let before_rejection = database.bootstrap_snapshot().unwrap();
    let error = database
        .archive_chat(&request(&project.id, &first_chat_id, &before_rejection))
        .unwrap_err();
    assert!(error.to_string().contains("last attached Chat"));
    let after_rejection = database.bootstrap_snapshot().unwrap();
    assert_eq!(
        after_rejection
            .tabs
            .iter()
            .map(|tab| &tab.id)
            .collect::<Vec<_>>(),
        before_rejection
            .tabs
            .iter()
            .map(|tab| &tab.id)
            .collect::<Vec<_>>()
    );
    assert_eq!(
        after_rejection.archived_chats,
        before_rejection.archived_chats
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn archived_projection_accepts_exact_limit_and_rejects_plus_one() {
    let root = std::env::temp_dir().join(format!("sage-chat-archive-limit-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    for ordinal in 1..=MAX_ARCHIVED_CHATS {
        insert_detached_chat(
            &database,
            &project.id,
            &format!("chat-archived-{ordinal:02}"),
            ordinal as i64,
        );
    }
    let exact = database.bootstrap_snapshot().unwrap();
    assert_eq!(exact.archived_chats.len(), MAX_ARCHIVED_CHATS);
    assert_eq!(exact.archived_chats[0].id, "chat-archived-01");
    assert_eq!(
        exact.archived_chats[MAX_ARCHIVED_CHATS - 1].id,
        "chat-archived-64"
    );

    insert_detached_chat(
        &database,
        &project.id,
        "chat-archived-65",
        MAX_ARCHIVED_CHATS as i64 + 1,
    );
    assert!(matches!(
        database.bootstrap_snapshot(),
        Err(AppError::ResourceLimit(_))
    ));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn restore_rejects_the_plus_one_tab_without_mutation() {
    let root = std::env::temp_dir().join(format!("sage-chat-restore-limit-{}", Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = initial.active_tab_id.clone().unwrap();
    let archived_chat_id = database
        .create_chat(&request(&project.id, &first_chat_id, &initial).current_layout)
        .unwrap();
    let attached = database.bootstrap_snapshot().unwrap();
    database
        .archive_chat(&request(&project.id, &archived_chat_id, &attached))
        .unwrap();

    let mut full_layout = request(
        &project.id,
        &first_chat_id,
        &database.bootstrap_snapshot().unwrap(),
    )
    .current_layout;
    for position in 1..MAX_RESTORED_TABS {
        full_layout.tabs.push(WorkbenchTab {
            id: format!("terminal-{position}"),
            kind: "terminal".into(),
            title: format!("Terminal {position}"),
            position: position as i64,
            pinned: false,
            dirty: false,
            status: Some("idle".into()),
            updated_at: None,
            resource: None,
            repository: None,
            diff_ref: None,
        });
    }
    database.persist_project_layout(&full_layout).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let error = database
        .restore_chat(&request(&project.id, &archived_chat_id, &before))
        .unwrap_err();
    assert!(matches!(error, AppError::ResourceLimit(_)));
    let after = database.bootstrap_snapshot().unwrap();
    assert_lifecycle_state_unchanged(&before, &after);
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn foreign_restore_and_storage_failures_leave_authoritative_state_unchanged() {
    let base = std::env::temp_dir().join(format!("sage-chat-rollback-{}", Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first_project = database.create_project_from_root(&first_root).unwrap();
    let first_initial = database.bootstrap_snapshot().unwrap();
    let first_chat_id = first_initial.active_tab_id.clone().unwrap();
    let second_chat_id = database
        .create_chat(&request(&first_project.id, &first_chat_id, &first_initial).current_layout)
        .unwrap();
    let second_project = database.create_project_from_root(&second_root).unwrap();
    let second_snapshot = database.bootstrap_snapshot().unwrap();
    let foreign_chat_id = second_snapshot.active_tab_id.clone().unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            current_layout: request(&second_project.id, &foreign_chat_id, &second_snapshot)
                .current_layout,
            target_project_id: first_project.id.clone(),
        })
        .unwrap();
    let before_foreign = database.bootstrap_snapshot().unwrap();
    assert!(
        database
            .restore_chat(&request(
                &first_project.id,
                &foreign_chat_id,
                &before_foreign,
            ))
            .is_err()
    );
    assert_lifecycle_state_unchanged(&before_foreign, &database.bootstrap_snapshot().unwrap());

    database
        .connection()
        .execute_batch(
            "CREATE TRIGGER fail_chat_detach
             BEFORE DELETE ON chat_tabs
             BEGIN SELECT RAISE(ABORT, 'injected archive failure'); END;",
        )
        .unwrap();
    assert!(
        database
            .archive_chat(&request(&first_project.id, &first_chat_id, &before_foreign,))
            .is_err()
    );
    assert_lifecycle_state_unchanged(&before_foreign, &database.bootstrap_snapshot().unwrap());
    database
        .connection()
        .execute_batch("DROP TRIGGER fail_chat_detach")
        .unwrap();

    database
        .archive_chat(&request(&first_project.id, &first_chat_id, &before_foreign))
        .unwrap();
    let before_restore = database.bootstrap_snapshot().unwrap();
    database
        .connection()
        .execute_batch(&format!(
            "CREATE TRIGGER fail_chat_attach
             BEFORE INSERT ON chat_tabs
             WHEN NEW.chat_id='{first_chat_id}'
             BEGIN SELECT RAISE(ABORT, 'injected restore failure'); END;"
        ))
        .unwrap();
    assert!(
        database
            .restore_chat(&request(&first_project.id, &first_chat_id, &before_restore,))
            .is_err()
    );
    assert_lifecycle_state_unchanged(&before_restore, &database.bootstrap_snapshot().unwrap());
    assert_eq!(
        before_restore.active_tab_id.as_deref(),
        Some(second_chat_id.as_str())
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn archives_a_chat_on_a_non_active_project_without_switching() {
    let base = std::env::temp_dir().join(format!("sage-chat-archive-foreign-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let seeded = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let extra = database
        .create_chat(
            &request(&cross.id, &seeded, &database.bootstrap_snapshot().unwrap()).current_layout,
        )
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: cross.id.clone(),
            chat_id: extra.clone(),
            current_layout: request(
                &sage.id,
                before.active_tab_id.as_deref().unwrap_or(""),
                &before,
            )
            .current_layout,
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    assert!(!cross_item.rail_chats.iter().any(|chat| chat.id == extra));
    assert!(cross_item.rail_chats.iter().any(|chat| chat.id == seeded));
    assert!(cross_item.rail_archived.iter().any(|chat| chat.id == extra));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn restores_a_chat_on_a_non_active_project_without_switching() {
    let base = std::env::temp_dir().join(format!("sage-chat-restore-foreign-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let seeded = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let extra = database
        .create_chat(
            &request(&cross.id, &seeded, &database.bootstrap_snapshot().unwrap()).current_layout,
        )
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let sage_layout = request(
        &sage.id,
        before.active_tab_id.as_deref().unwrap_or(""),
        &before,
    )
    .current_layout;
    database
        .archive_chat(&ChatLifecycleRequest {
            project_id: cross.id.clone(),
            chat_id: extra.clone(),
            current_layout: sage_layout.clone(),
        })
        .unwrap();
    database
        .restore_chat(&ChatLifecycleRequest {
            project_id: cross.id.clone(),
            chat_id: extra.clone(),
            current_layout: sage_layout,
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    assert!(cross_item.rail_chats.iter().any(|chat| chat.id == extra));
    assert!(!cross_item.rail_archived.iter().any(|chat| chat.id == extra));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn refuses_to_archive_the_last_chat_on_a_non_active_project() {
    let base = std::env::temp_dir().join(format!("sage-chat-archive-last-{}", Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let seeded = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    let error = database
        .archive_chat(&ChatLifecycleRequest {
            project_id: cross.id,
            chat_id: seeded,
            current_layout: request(
                &sage.id,
                before.active_tab_id.as_deref().unwrap_or(""),
                &before,
            )
            .current_layout,
        })
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidRequest(_)));
    fs::remove_dir_all(base).unwrap();
}
