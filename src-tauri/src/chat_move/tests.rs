use std::fs;

use crate::{
    domain::{MoveChatRequest, ProjectLayoutRequest, ProjectSwitchRequest},
    error::AppError,
    persistence::Database,
};

fn layout(snapshot: &crate::domain::BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn moves_an_attached_chat_to_another_project() {
    let base = std::env::temp_dir().join(format!("sage-chat-move-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first = database.create_project_from_root(&first_root).unwrap();
    let first_chat = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let second_chat = database
        .create_chat(&layout(&database.bootstrap_snapshot().unwrap()))
        .unwrap();
    let second = database.create_project_from_root(&second_root).unwrap();
    database
        .switch_project(&ProjectSwitchRequest {
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
            target_project_id: first.id.clone(),
        })
        .unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .move_chat(&MoveChatRequest {
            project_id: first.id.clone(),
            chat_id: second_chat.clone(),
            target_project_id: second.id.clone(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, second.id);
    assert!(after.tabs.iter().any(|tab| tab.id == second_chat));
    assert!(!after.tabs.iter().any(|tab| tab.id == first_chat));
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn refuses_same_project_and_unknown_target() {
    let root = std::env::temp_dir().join(format!("sage-chat-move-deny-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let chat_id = snapshot.active_tab_id.clone().unwrap();
    let same = database
        .move_chat(&MoveChatRequest {
            project_id: project.id.clone(),
            chat_id: chat_id.clone(),
            target_project_id: project.id.clone(),
            current_layout: layout(&snapshot),
        })
        .unwrap_err();
    assert!(matches!(same, AppError::InvalidRequest(_)));
    let unknown = database
        .move_chat(&MoveChatRequest {
            project_id: project.id,
            chat_id,
            target_project_id: "missing-project".into(),
            current_layout: layout(&snapshot),
        })
        .unwrap_err();
    assert!(matches!(unknown, AppError::InvalidRequest(_)));
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn moves_a_sessionless_chat_from_a_non_active_project_without_switching() {
    let base =
        std::env::temp_dir().join(format!("sage-chat-move-foreign-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("work");
    let third_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    fs::create_dir_all(&third_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let cross_chat = database
        .create_chat(&layout(&database.bootstrap_snapshot().unwrap()))
        .unwrap();
    let work = database.create_project_from_root(&second_root).unwrap();
    let sage = database.create_project_from_root(&third_root).unwrap();
    let before = database.bootstrap_snapshot().unwrap();
    database
        .move_chat(&MoveChatRequest {
            project_id: cross.id.clone(),
            chat_id: cross_chat.clone(),
            target_project_id: work.id.clone(),
            current_layout: layout(&before),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let work_item = after
        .projects
        .iter()
        .find(|item| item.id == work.id)
        .unwrap();
    assert!(
        work_item
            .rail_chats
            .iter()
            .any(|chat| chat.id == cross_chat)
    );
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    assert!(
        !cross_item
            .rail_chats
            .iter()
            .any(|chat| chat.id == cross_chat)
    );
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn refuses_to_move_a_chat_that_has_an_agent_session() {
    let base =
        std::env::temp_dir().join(format!("sage-chat-move-session-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("first");
    let second_root = base.join("second");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let first = database.create_project_from_root(&first_root).unwrap();
    let chat_id = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    let second = database.create_project_from_root(&second_root).unwrap();
    let error = database
        .move_chat(&MoveChatRequest {
            project_id: first.id,
            chat_id,
            target_project_id: second.id,
            current_layout: layout(&database.bootstrap_snapshot().unwrap()),
        })
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidRequest(_)));
    assert!(error.to_string().contains("agent session"));
    fs::remove_dir_all(base).unwrap();
}
