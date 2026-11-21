use super::*;
use crate::acp::store::{StartSessionCommand, load_chat_page, start_session};
use crate::chat_lifecycle::create::ChatCreateStore;
use crate::project_registry::create::ProjectCreateStore;
use crate::session_events::SessionEventStore;
use sage_acp_protocol::{
    contract::{ClientIntentId, ProjectionRevision, SageChatId, SageProjectId},
    intent::BindingId,
};

#[test]
fn release_like_draft_fallback_uses_recent_available_binding_despite_simulator_history() {
    use super::super::bootstrap::restore_harness_usage;
    let (database, project_id, legacy_chat) = database_with_project();
    seed_closed_simulator_history(&database, &legacy_chat, "legacy prompt", "legacy answer");
    let catalog = HarnessCatalog::default();
    let template = catalog
        .definitions()
        .into_iter()
        .find(|harness| harness.transport == sage_domain::harness::HarnessTransport::Acp)
        .unwrap();
    // A release-like catalog contains only these two admitted bindings, with
    // no launch entry for the earlier chat's sage-simulator binding.
    let mut definitions = Vec::new();
    for (ordinal, id, used_at) in [(1, "first", 10), (2, "recent", 20)] {
        let mut harness = template.clone();
        harness.id = id.into();
        harness.binding_id = Some(format!("binding-{id}"));
        harness.install_state = "available".into();
        definitions.push(harness);
        insert_empty_chat(&database, &project_id, id, ordinal);
        start_session(
            &mut database.write_connection().unwrap(),
            StartSessionCommand {
                project_id: SageProjectId::parse(&project_id).unwrap(),
                chat_id: SageChatId::parse(id).unwrap(),
                expected_projection_revision: ProjectionRevision::new(0).unwrap(),
                client_intent_id: ClientIntentId::parse(format!("start-{id}")).unwrap(),
                binding_id: BindingId::new("bindingId", format!("binding-{id}")).unwrap(),
                binding_digest: "a".repeat(64),
                harness_version: None,
                model_id: None,
                now_millis: used_at,
            },
        )
        .unwrap();
    }
    assert_eq!(
        HarnessCatalog::preferred_available_harness_id(&definitions).as_deref(),
        Some("first")
    );
    restore_harness_usage(&database.connection(), &mut definitions).unwrap();
    assert_eq!(
        HarnessCatalog::preferred_available_harness_id(&definitions).as_deref(),
        Some("recent")
    );
    definitions[1].install_state = "unavailable".into();
    assert_eq!(
        HarnessCatalog::preferred_available_harness_id(&definitions).as_deref(),
        Some("first")
    );
    definitions[0].install_state = "unavailable".into();
    assert_eq!(
        HarnessCatalog::preferred_available_harness_id(&definitions),
        None
    );
    let page = load_chat_page(
        &database.connection(),
        SageProjectId::parse(project_id).unwrap(),
        SageChatId::parse(legacy_chat).unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        page.sessions.as_slice()[0].binding_id.as_str(),
        "sage-simulator"
    );
    assert_eq!(page.turns.as_slice().len(), 1);
}

#[test]
fn unknown_persisted_binding_restores_as_unavailable_without_losing_the_chat() {
    let base = std::env::temp_dir().join(format!("sage-unknown-binding-{}", Uuid::new_v4()));
    let root = base.join("root");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = crate::database::open_database(&path).unwrap();
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

    let reopened = crate::database::open_database(&path).unwrap();
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
        if crate::harnesses::SIMULATOR_IN_CATALOG {
            Some(crate::harnesses::SIMULATOR_ID.into())
        } else {
            HarnessCatalog::preferred_available_harness_id(&catalog.definitions())
        }
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
