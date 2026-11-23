use super::*;
use crate::acp::{bindings::codex::CodexBindingRegistration, mcp::McpAdmission};
use crate::project_registry::create::ProjectCreateStore;
use crate::workbench_persistence::store::WorkbenchStore;
use sage_acp_protocol::contract::{ClientIntentId, RuntimeEpoch, SageChatId, SageProjectId};

/// Regression: a harness whose installed files changed after admission could
/// not be launched, yet the chat said its session identity was stale or foreign.
#[test]
fn a_harness_that_cannot_be_launched_says_so_instead_of_stale_identity() {
    tauri::async_runtime::block_on(async {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .canonicalize()
            .unwrap();
        let fixture_root = root
            .join("work")
            .join(format!("codex-launch-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&fixture_root).unwrap();
        let executable = fixture_root.canonicalize().unwrap().join("codex.exe");
        std::fs::write(&executable, b"@echo off\r\n").unwrap();
        let registration = CodexBindingRegistration::unavailable_for_test();
        registration
            .authenticate_observed_for_test(executable.clone(), "codex-cli 0.166.0".into())
            .unwrap();
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        let project = database.create_project_with_chat(&root).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let project_id = SageProjectId::parse(project.id).unwrap();
        let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
        let service = AcpService::new_with_codex_for_test(database, registration).unwrap();
        let revision = service
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let project_root =
            super::super::tests::codex_root(&project_id, &project.roots[0].id, &root);
        let started = service
            .start_codex_session(
                &project_root,
                super::super::tests::codex_start_command(&project_id, &chat_id, revision),
            )
            .await
            .unwrap();
        let admitted = service
            .binding_registry()
            .resolve("codex-app-server")
            .unwrap();
        // The admitted executable changes on disk after admission, so only
        // constructing the launch notices it.
        std::fs::write(&executable, b"@echo off\r\nrem replaced\r\n").unwrap();
        let mut command = ActivateSessionCommand {
            project_id,
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "forged".into(),
            now_millis: 1,
        };
        let refused = service
            .prepare_session_activation_inner(
                admitted,
                &mut command,
                crate::acp::bindings::registry::BindingProjectRoot::Codex(project_root),
                |_| (McpAdmission::default(), None),
            )
            .err()
            .expect("a changed harness must not launch");
        assert!(
            matches!(
                refused,
                CodexSessionError::LaunchSetup(
                    crate::acp::service::CodexLaunchSetupError::RuntimeConstruction
                )
            ),
            "{refused:?}"
        );
        service.shutdown_blocking().unwrap();
        std::fs::remove_dir_all(fixture_root).unwrap();
    });
}
