use super::*;
use crate::acp::{
    contract::{ClientIntentId, ProjectionRevision, SageChatId, SageProjectId},
    intent::BindingId,
    store::{StartSessionCommand, load_chat_page, start_session},
};

#[test]
fn unknown_persisted_binding_restores_as_unavailable_without_losing_the_chat() {
    let base = std::env::temp_dir().join(format!("sage-unknown-binding-{}", Uuid::new_v4()));
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let initial = database.bootstrap_snapshot().unwrap();
    let chat_id = initial.active_tab_id.unwrap();
    seed_closed_simulator_history(&database, &chat_id, "preserved prompt", "preserved answer");
    let catalog = HarnessCatalog::default();
    assert!(catalog.harness_id_for_binding("absent-binding").is_none());
    let revision: i64 = database
        .connection()
        .query_row(
            "SELECT projection_revision FROM chats WHERE id=?1",
            [&chat_id],
            |row| row.get(0),
        )
        .unwrap();
    let started = start_session(
        &mut database.write_connection().unwrap(),
        StartSessionCommand {
            project_id: SageProjectId::parse(&project.id).unwrap(),
            chat_id: SageChatId::parse(&chat_id).unwrap(),
            expected_projection_revision: ProjectionRevision::new(revision as u64).unwrap(),
            client_intent_id: ClientIntentId::parse("unknown-binding-start").unwrap(),
            binding_id: BindingId::new("bindingId", "absent-binding").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: now_millis(),
        },
    )
    .unwrap();
    drop(database);

    let reopened = Database::open(&path).unwrap();
    let snapshot = reopened.bootstrap_snapshot_with_catalog(&catalog).unwrap();
    assert_eq!(snapshot.project.as_ref().unwrap().id, project.id);
    assert_eq!(snapshot.active_tab_id.as_deref(), Some(chat_id.as_str()));
    assert!(
        snapshot
            .tabs
            .iter()
            .any(|tab| tab.id == chat_id && tab.kind == "chat")
    );
    assert_eq!(
        snapshot.session_id.as_deref(),
        Some(started.session_id.as_str())
    );
    assert_eq!(snapshot.messages.len(), 2);
    assert_eq!(snapshot.messages[0].text, "preserved prompt");
    assert_eq!(snapshot.messages[1].text, "preserved answer");
    let unavailable = snapshot
        .harnesses
        .iter()
        .find(|harness| Some(&harness.id) == snapshot.selected_harness_id.as_ref())
        .unwrap();
    assert_eq!(unavailable.binding_id.as_deref(), Some("absent-binding"));
    assert_eq!(unavailable.install_state, "unavailable");
    unavailable.validate_transport_contract().unwrap();

    reopened
        .create_chat(&ProjectLayoutRequest {
            project_id: project.id,
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        })
        .unwrap();
    let fresh = reopened.bootstrap_snapshot_with_catalog(&catalog).unwrap();
    assert_eq!(
        fresh.selected_harness_id,
        catalog.default_selected_harness_id()
    );
    assert!(fresh.harnesses.iter().any(|harness| {
        harness.binding_id.as_deref() == Some("absent-binding")
            && harness.install_state == "unavailable"
    }));
    assert_eq!(reopened.messages_for_chat(&chat_id).unwrap().len(), 2);
    let page = load_chat_page(
        &reopened.connection(),
        SageProjectId::parse(fresh.project.unwrap().id).unwrap(),
        SageChatId::parse(chat_id).unwrap(),
        None,
    )
    .unwrap();
    let persisted = page
        .sessions
        .as_slice()
        .iter()
        .find(|session| session.id == started.session_id)
        .unwrap();
    assert_eq!(persisted.binding_id.as_str(), "absent-binding");
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}
