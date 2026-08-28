use std::fs;

use crate::{
    domain::{PinChatRequest, ProjectLayoutRequest},
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
fn pins_a_chat_on_a_non_active_project_without_switching() {
    let base = std::env::temp_dir().join(format!("sage-chat-pin-{}", uuid::Uuid::new_v4()));
    let first_root = base.join("cross");
    let second_root = base.join("sage");
    fs::create_dir_all(&first_root).unwrap();
    fs::create_dir_all(&second_root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let cross = database.create_project_from_root(&first_root).unwrap();
    let cross_chat = database
        .create_chat(&layout(&database.bootstrap_snapshot().unwrap()))
        .unwrap();
    let sage = database.create_project_from_root(&second_root).unwrap();
    database
        .pin_chat(&PinChatRequest {
            project_id: cross.id.clone(),
            chat_id: cross_chat.clone(),
        })
        .unwrap();
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.as_ref().unwrap().id, sage.id);
    let cross_item = after
        .projects
        .iter()
        .find(|item| item.id == cross.id)
        .unwrap();
    let pinned = cross_item
        .rail_chats
        .iter()
        .find(|chat| chat.id == cross_chat)
        .unwrap();
    assert!(pinned.pinned);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn refuses_an_unknown_chat_pin() {
    let root = std::env::temp_dir().join(format!("sage-chat-pin-deny-{}", uuid::Uuid::new_v4()));
    fs::create_dir_all(&root).unwrap();
    let database = Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let error = database
        .pin_chat(&PinChatRequest {
            project_id: project.id,
            chat_id: "missing-chat".into(),
        })
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidRequest(_)));
    fs::remove_dir_all(root).unwrap();
}
