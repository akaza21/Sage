use crate::{
    acp::{
        bindings::registry::BindingRegistry,
        contract::{ProjectGeneration, SageProjectId},
        intent::{ACP_PROJECTION_CHANGED_EVENT, ProjectionChangedV1},
        runtime::{NotificationError, NotificationSink, NotificationSinkFuture},
        service::AcpService,
    },
    editor_coordination::EditorOperationCoordinator,
    error::AppResult,
    harnesses::HarnessCatalog,
    persistence::Database,
    settings_store::SettingsStore,
};
use tauri::Manager;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ActiveProjectError {
    GenerationUnavailable,
    StaleGeneration,
    ForeignProject,
    Storage,
}

/// Linear proof that a project ID and generation are still authoritative.
/// Owning the transition guard prevents a project switch until the native
/// operation has finished resolving all project-scoped capabilities.
pub(crate) struct ActiveProjectContext<'a> {
    _transition: tokio::sync::MutexGuard<'a, u64>,
    project_id: SageProjectId,
    generation: ProjectGeneration,
}

impl ActiveProjectContext<'_> {
    pub(crate) fn project_id(&self) -> &SageProjectId {
        &self.project_id
    }

    pub(crate) fn generation(&self) -> ProjectGeneration {
        self.generation
    }
}

#[derive(Debug)]
pub struct AppState {
    pub database: Database,
    pub settings: SettingsStore,
    pub acp: AcpService,
    pub harnesses: HarnessCatalog,
    pub harness_version_cache: crate::harness_discovery::DisplayVersionCache,
    pub editor_operations: EditorOperationCoordinator,
    pub project_transitions: tokio::sync::Mutex<u64>,
    pub startup_recovery_notice: Option<String>,
    pub terminals: crate::user_terminal::TerminalSessionPort,
}

impl AppState {
    pub(crate) fn refresh_harnesses_after_window_shown(app: tauri::AppHandle) {
        tauri::async_runtime::spawn_blocking(move || {
            let presence = crate::harness_discovery::discover_installed_harness_presence();
            let state = app.state::<AppState>();
            state.harnesses.apply_presence(presence.clone());
            use tauri::Emitter;
            let _ = app.emit(
                "harness-discovery-changed-v1",
                state.harnesses.definitions(),
            );
            let (claude, discovered) = std::thread::scope(|scope| {
                let claude = scope.spawn(crate::harness_discovery::discover_claude_launch);
                let discovered = crate::harness_discovery::enrich_installed_harness_versions(
                    &presence,
                    &state.harness_version_cache,
                );
                let claude = claude.join().unwrap_or_else(|_| {
                    tracing::warn!("Claude launch discovery panicked");
                    crate::harness_discovery::ClaudeLaunchDiscovery::AdapterVersionUnreadable
                });
                (claude, discovered)
            });
            state.harnesses.apply_discovery(claude, discovered);
            let _ = app.emit(
                "harness-discovery-changed-v1",
                state.harnesses.definitions(),
            );
        });
    }

    pub(crate) async fn lock_active_project(
        &self,
        project_id: &SageProjectId,
        generation: ProjectGeneration,
    ) -> Result<ActiveProjectContext<'_>, ActiveProjectError> {
        let transition = self.project_transitions.lock().await;
        let expected = ProjectGeneration::new(*transition)
            .map_err(|_| ActiveProjectError::GenerationUnavailable)?;
        if generation != expected {
            return Err(ActiveProjectError::StaleGeneration);
        }
        let active = self
            .database
            .active_project_id()
            .map_err(|_| ActiveProjectError::Storage)?;
        if active.as_deref() != Some(project_id.as_str()) {
            return Err(ActiveProjectError::ForeignProject);
        }
        Ok(ActiveProjectContext {
            _transition: transition,
            project_id: project_id.clone(),
            generation,
        })
    }

    #[cfg(test)]
    pub fn try_new(database: Database) -> AppResult<Self> {
        let settings = SettingsStore::in_memory()?;
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let registry = std::sync::Arc::new(BindingRegistry::default());
        let acp = AcpService::new_with_registry_for_test(database.clone(), registry.clone())
            .map_err(|_| {
                crate::error::AppError::InvalidRequest(
                    "bounded ACP restart recovery failed; durable history was preserved".into(),
                )
            })?;
        Ok(Self {
            database,
            settings,
            acp,
            harnesses: HarnessCatalog::with_registry(registry),
            harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
            terminals: crate::user_terminal::TerminalSessionPort::desktop(),
        })
    }

    pub fn try_new_desktop(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
    ) -> AppResult<Self> {
        Self::try_new_desktop_with_registry(
            database,
            settings,
            app,
            std::sync::Arc::new(BindingRegistry::default()),
            false,
        )
    }

    fn try_new_desktop_with_registry(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
        registry: std::sync::Arc<BindingRegistry>,
        fixture_isolation: bool,
    ) -> AppResult<Self> {
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let acp = AcpService::new_with_notification_sink_and_registry(
            database.clone(),
            std::sync::Arc::new(DesktopProjectionSink(app.clone())),
            registry.clone(),
        )
        .map_err(|_| {
            crate::error::AppError::InvalidRequest(
                "bounded ACP restart recovery failed; durable history was preserved".into(),
            )
        })?;
        #[cfg(windows)]
        if !fixture_isolation {
            acp.set_browser_app(app.clone());
        }
        let goal_app = app.clone();
        acp.set_session_config_notice_sink(std::sync::Arc::new(move |notice| {
            use tauri::Emitter;
            let _ = app.emit("acp-session-config-changed-v1", notice);
        }));
        acp.set_goal_notice_sink(std::sync::Arc::new(move |notice| {
            use tauri::Emitter;
            let _ = goal_app.emit("acp-goal-changed-v1", notice);
        }));
        let harnesses = HarnessCatalog::with_registry(registry);
        if fixture_isolation {
            harnesses.apply_presence(Vec::new());
        }
        Ok(Self {
            database,
            settings,
            acp,
            harnesses,
            harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
            terminals: crate::user_terminal::TerminalSessionPort::desktop(),
        })
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub fn try_new_characterization_desktop(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
        binding: crate::acp::service::CharacterizationBinding,
    ) -> AppResult<Self> {
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let registry = std::sync::Arc::new(BindingRegistry::default());
        let acp = AcpService::new_with_characterization_sink(
            database.clone(),
            std::sync::Arc::new(DesktopProjectionSink(app.clone())),
            binding,
            registry.clone(),
        )
        .map_err(|_| {
            crate::error::AppError::InvalidRequest(
                "bounded ACP restart recovery failed; durable history was preserved".into(),
            )
        })?;
        acp.set_browser_app(app);
        Ok(Self {
            database,
            settings,
            acp,
            harnesses: HarnessCatalog::with_registry_and_characterization(registry),
            harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
            terminals: crate::user_terminal::TerminalSessionPort::desktop(),
        })
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn try_new_characterization(
        database: Database,
        binding: crate::acp::service::CharacterizationBinding,
    ) -> AppResult<Self> {
        let settings = SettingsStore::in_memory()?;
        settings.editor_appearance()?;
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let registry = std::sync::Arc::new(BindingRegistry::default());
        let acp =
            AcpService::new_characterization_for_test(database.clone(), binding, registry.clone())
                .map_err(|_| {
                    crate::error::AppError::InvalidRequest(
                        "bounded ACP restart recovery failed; durable history was preserved".into(),
                    )
                })?;
        Ok(Self {
            database,
            settings,
            acp,
            harnesses: HarnessCatalog::with_registry_and_characterization(registry),
            harness_version_cache: crate::harness_discovery::DisplayVersionCache::default(),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
            terminals: crate::user_terminal::TerminalSessionPort::desktop(),
        })
    }

    #[cfg(test)]
    pub fn new(database: Database) -> Self {
        Self::try_new(database).expect("test ACP state must recover")
    }
}

struct DesktopProjectionSink(tauri::AppHandle);

impl NotificationSink for DesktopProjectionSink {
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            use tauri::Emitter;

            self.0
                .emit(ACP_PROJECTION_CHANGED_EVENT, event)
                .map_err(|_| NotificationError::SinkUnavailable)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::contract::{ProjectGeneration, SageProjectId};

    #[test]
    fn app_state_shares_one_codex_registration_with_service_and_catalog() {
        let state = AppState::new(Database::open(std::path::Path::new(":memory:")).unwrap());
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
        use crate::acp::{
            bindings::{codex::CodexAdmissionUnavailable, registry::RegistryUnavailable},
            intent::BindingId,
        };

        let state = AppState::new(Database::open(std::path::Path::new(":memory:")).unwrap());
        let registry = state.acp.binding_registry();
        for id in ["unknown", "pi", "oh-my-pi"] {
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
        for id in ["pi", "oh-my-pi"] {
            let entry = definitions.iter().find(|entry| entry.id == id).unwrap();
            assert!(entry.binding_id.is_none());
            assert_ne!(entry.install_state, "available");
        }
    }

    #[test]
    fn active_project_context_rejects_stale_generation_and_foreign_project() {
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            let database = Database::open(std::path::Path::new(":memory:")).unwrap();
            let root =
                std::env::temp_dir().join(format!("sage-active-context-{}", uuid::Uuid::new_v4()));
            std::fs::create_dir_all(&root).unwrap();
            let project = database.create_project_from_root(&root).unwrap();
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
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[path = "state_fixture.rs"]
mod desktop_fixture;
