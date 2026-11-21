use super::*;
use crate::acp::{
    bindings::{
        claude::ClaudeBindingRegistration,
        codex::CodexBindingRegistration,
        registry::{BindingProjectRoot, BindingRegistry},
    },
    runtime::ActivationMode,
    store::{ActivateSessionCommand, StartSessionCommand},
};
use crate::harness_discovery::{ClaudeLaunchDiscovery, DiscoveredClaudeLaunch};
use crate::{
    project_registry::{ProjectRegistryStore, create::ProjectCreateStore},
    workbench_persistence::store::WorkbenchStore,
};
use sage_acp_protocol::{
    contract::{ClientIntentId, ProjectGeneration, RuntimeEpoch, SageChatId, SageProjectId},
    intent::BindingId,
};
use std::{fs, sync::Arc};

#[cfg(all(windows, feature = "acp-fixture"))]
#[path = "private_chat_tests/live.rs"]
mod live;

#[test]
fn private_chat_session_uses_its_registered_folder_and_freezes_scope_before_a_turn() {
    tauri::async_runtime::block_on(async {
        let root = std::env::current_dir()
            .unwrap()
            .join("work")
            .join(format!("private-session-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&root).unwrap();
        let adapter = root.join("adapter.js");
        let node = root.join("node.exe");
        fs::write(&adapter, b"adapter").unwrap();
        fs::write(&node, b"node").unwrap();
        let registration = ClaudeBindingRegistration::admitted_for_test(
            ClaudeLaunchDiscovery::Present(Box::new(DiscoveredClaudeLaunch {
                adapter_path: adapter.clone(),
                adapter_version: "0.76.0".into(),
                acp_sdk_version: "1.4.0".into(),
                node_engine: ">=22".into(),
                node_path: node.clone(),
                node_version: "22.18.0".into(),
                adapter_sha256: crate::harness_discovery::sha256_file(&adapter).unwrap(),
                node_sha256: crate::harness_discovery::sha256_file(&node).unwrap(),
            })),
        );
        let database = crate::database::open_database(&root.join("sage.db")).unwrap();
        let real = database.create_project_from_root(&root).unwrap();
        let chat = uuid::Uuid::new_v4().to_string();
        crate::private_chat::create(&database, None, &chat).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project = snapshot.project.unwrap();
        let folder = database.root_records(&project.id).unwrap()[0]
            .absolute_path
            .clone();
        assert!(!folder.to_string_lossy().starts_with(r"\\?\"));
        let project_id = SageProjectId::parse(project.id.clone()).unwrap();
        let chat_id = SageChatId::parse(chat.clone()).unwrap();
        let state = crate::state::AppState::new(database.clone());
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::ZERO)
            .await
            .unwrap();
        let authority = database.selected_project_root_authority(&context).unwrap();
        let service = AcpService::new_with_registry_for_test(
            database.clone(),
            Arc::new(BindingRegistry::new(
                CodexBindingRegistration::unavailable_for_test(),
                registration,
            )),
        )
        .unwrap();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = service
            .start_session(
                service
                    .binding_registry()
                    .resolve("claude-agent-acp")
                    .unwrap(),
                &BindingProjectRoot::Claude(authority),
                StartSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: revision,
                    client_intent_id: ClientIntentId::new(),
                    binding_id: BindingId::new("bindingId", "claude-agent-acp").unwrap(),
                    binding_digest: "test".into(),
                    harness_version: None,
                    model_id: None,
                    now_millis: sage_core::time::now_millis(),
                },
            )
            .await
            .unwrap();
        let authority = database.selected_project_root_authority(&context).unwrap();
        let (_, prepared) = service
            .prepare_session_activation(
                service
                    .binding_registry()
                    .resolve("claude-agent-acp")
                    .unwrap(),
                ActivateSessionCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    expected_projection_revision: started.projection_revision,
                    client_intent_id: ClientIntentId::new(),
                    session_id: started.session_id,
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    artifact_digest: "test".into(),
                    now_millis: sage_core::time::now_millis(),
                },
                BindingProjectRoot::Claude(authority),
            )
            .unwrap();
        let ActivationMode::New { native_root, .. } = &prepared.intent.mode else {
            panic!("expected a new session");
        };
        assert!(!native_root.starts_with(r"\\?\"));
        assert_eq!(
            crate::project_registry::authority_path_key(
                &std::path::Path::new(native_root).canonicalize().unwrap(),
            ),
            crate::project_registry::authority_path_key(&folder.canonicalize().unwrap()),
        );
        assert!(
            service
                .load_chat_page(project_id, chat_id, None)
                .unwrap()
                .turns
                .as_slice()
                .is_empty()
        );
        drop(context);
        let current = sage_domain::chat::ProjectLayoutRequest {
            project_id: project.id,
            tabs: snapshot.tabs,
            active_tab_id: snapshot.active_tab_id,
            selected_root_id: snapshot.selected_root_id,
            selected_resource: snapshot.selected_resource,
        };
        assert!(
            crate::private_chat::set_project(&database, &current, &chat, Some(&real.id)).is_err()
        );
        drop(prepared);
        service.shutdown_blocking().unwrap();
        drop(service);
        drop(state);
        drop(database);
        fs::remove_dir_all(root).unwrap();
    });
}
