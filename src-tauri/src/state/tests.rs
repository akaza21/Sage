use super::*;
use crate::project_registry::create::ProjectCreateStore;
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId};

#[test]
fn app_state_shares_one_codex_registration_with_service_and_catalog() {
    let state =
        AppState::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap());
    assert!(std::sync::Arc::ptr_eq(
        state.acp.binding_registry(),
        state.harnesses.binding_registry(),
    ));
    assert!(
        state
            .acp
            .codex_registration()
            .same_authority(&state.harnesses.codex_registration())
    );
}

#[test]
fn shared_registry_rejects_unknown_dormant_and_unavailable_launch_authority() {
    use crate::acp::bindings::{codex::CodexAdmissionUnavailable, registry::RegistryUnavailable};
    use sage_acp_protocol::intent::BindingId;

    let state =
        AppState::new(crate::database::open_database(std::path::Path::new(":memory:")).unwrap());
    let registry = state.acp.binding_registry();
    for id in ["unknown", "oh-my-pi"] {
        assert!(registry.for_harness(id).is_none());
        assert!(matches!(
            registry.resolve(id),
            Err(RegistryUnavailable::UnknownBinding)
        ));
        assert!(
            !state
                .acp
                .accepts_codex_binding(&BindingId::new("bindingId", id).unwrap())
        );
    }
    assert!(registry.for_harness("claude-code").is_some());
    assert!(!matches!(
        registry.resolve("claude-agent-acp"),
        Err(RegistryUnavailable::UnknownBinding)
    ));
    assert!(
        !state
            .acp
            .accepts_codex_binding(&BindingId::new("bindingId", "claude-agent-acp").unwrap())
    );
    // A known catalog identity must not become launch authority after admission is lost.
    registry.codex_admission().invalidate_drift();
    assert!(matches!(
        registry.resolve("codex-app-server"),
        Err(RegistryUnavailable::Admission(
            CodexAdmissionUnavailable::Drift
        ))
    ));
    assert!(
        !state
            .acp
            .accepts_codex_binding(&BindingId::new("bindingId", "codex-app-server").unwrap(),)
    );
    let definitions = state.harnesses.definitions();
    let codex = definitions
        .iter()
        .find(|entry| entry.id == "codex")
        .unwrap();
    assert_ne!(codex.install_state, "available");
    assert_eq!(codex.binding_id.as_deref(), Some("codex-app-server"));
    let claude = definitions
        .iter()
        .find(|entry| entry.id == "claude-code")
        .unwrap();
    assert_eq!(claude.binding_id.as_deref(), Some("claude-agent-acp"));
    // Pi has a curated read-only binding that stays unadmitted until
    // discovery finds the installed package.
    assert!(registry.for_harness("pi").is_some());
    assert!(registry.resolve("pi-rpc").is_err());
    let pi = definitions.iter().find(|entry| entry.id == "pi").unwrap();
    assert_eq!(pi.binding_id.as_deref(), Some("pi-rpc"));
    assert_ne!(pi.install_state, "available");
    let omp = definitions
        .iter()
        .find(|entry| entry.id == "oh-my-pi")
        .unwrap();
    assert!(omp.binding_id.is_none());
    assert_ne!(omp.install_state, "available");
}

#[test]
fn active_project_context_rejects_stale_generation_and_foreign_project() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let database = crate::database::open_database(std::path::Path::new(":memory:")).unwrap();
        let root =
            std::env::temp_dir().join(format!("sage-active-context-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let project = database.create_project_with_chat(&root).unwrap();
        let state = AppState::new(database);
        let project_id = SageProjectId::parse(project.id).unwrap();

        assert!(matches!(
            state
                .lock_active_project(&project_id, ProjectGeneration::new(1).unwrap())
                .await,
            Err(ActiveProjectError::StaleGeneration)
        ));
        assert!(matches!(
            state
                .lock_active_project(
                    &SageProjectId::parse("project-foreign").unwrap(),
                    ProjectGeneration::new(0).unwrap(),
                )
                .await,
            Err(ActiveProjectError::ForeignProject)
        ));
        let context = state
            .lock_active_project(&project_id, ProjectGeneration::new(0).unwrap())
            .await
            .unwrap();
        assert_eq!(context.project_id(), &project_id);
        assert_eq!(context.generation(), ProjectGeneration::new(0).unwrap());

        drop(context);
        let _ = std::fs::remove_dir_all(root);
    });
}
