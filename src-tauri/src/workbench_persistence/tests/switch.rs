use super::*;

fn layout(snapshot: &BootstrapSnapshot) -> ProjectLayoutRequest {
    ProjectLayoutRequest {
        project_id: snapshot.project.as_ref().unwrap().id.clone(),
        tabs: snapshot.tabs.clone(),
        active_tab_id: snapshot.active_tab_id.clone(),
        selected_root_id: snapshot.selected_root_id.clone(),
        selected_resource: snapshot.selected_resource.clone(),
    }
}

#[test]
fn switching_back_to_a_project_without_roots_or_chats_restores_its_empty_layout() {
    let (database, other_id, _) = database_with_project();
    let empty = database
        .create_named_project_if_active("Empty", &[], Some(&other_id))
        .unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    assert!(initial.tabs.is_empty());
    assert!(initial.active_tab_id.is_none());
    let before = database.bootstrap_snapshot().unwrap();
    assert!(before.tabs.is_empty());
    assert!(before.archived_chats.is_empty());

    database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: other_id.clone(),
            current_layout: layout(&before),
        })
        .unwrap();
    let away = database.bootstrap_snapshot().unwrap();
    assert_eq!(away.project.as_ref().unwrap().id, other_id);
    database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: empty.id.clone(),
            current_layout: layout(&away),
        })
        .unwrap();

    let restored = database.bootstrap_snapshot().unwrap();
    assert_eq!(restored.project.as_ref().unwrap().id, empty.id);
    assert!(restored.project.unwrap().roots.is_empty());
    assert!(restored.tabs.is_empty());
    assert!(restored.archived_chats.is_empty());
    assert!(restored.active_tab_id.is_none());
    assert!(restored.selected_root_id.is_none());
    assert!(restored.selected_resource.is_none());
    assert!(restored.session_id.is_none());
    assert!(restored.messages.is_empty());
    assert!(restored.repositories.roots.is_empty());
}

#[test]
fn switching_to_an_unknown_project_rolls_back_the_outgoing_layout() {
    let (database, project_id, _) = database_with_project();
    let before = database.bootstrap_snapshot().unwrap();
    let mut outgoing = layout(&before);
    outgoing.tabs.clear();
    outgoing.active_tab_id = None;

    let error = database
        .switch_project(&ProjectSwitchRequest {
            target_project_id: "missing-project".into(),
            current_layout: outgoing,
        })
        .unwrap_err();

    assert!(error.to_string().contains("target project does not exist"));
    let after = database.bootstrap_snapshot().unwrap();
    assert_eq!(after.project.unwrap().id, project_id);
    assert_eq!(after.active_tab_id, before.active_tab_id);
    assert_eq!(after.tabs.len(), 1);
    assert_eq!(after.tabs[0].id, before.tabs[0].id);
    assert!(after.archived_chats.is_empty());
}
