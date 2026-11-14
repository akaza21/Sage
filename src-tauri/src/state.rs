use sage_core::error::AppResult;

use crate::{
    acp::{
        bindings::registry::BindingRegistry,
        contract::{ProjectGeneration, SageProjectId},
        intent::{ACP_PROJECTION_CHANGED_EVENT, ProjectionChangedV1},
        runtime::{NotificationError, NotificationSink, NotificationSinkFuture},
        service::AcpService,
    },
    editor_coordination::EditorOperationCoordinator,
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
    pub archive_releases: crate::chat_archive_commands::ArchiveReleases,
}

impl AppState {
    pub(crate) fn release_idle_runtimes_periodically(app: tauri::AppHandle) {
        tauri::async_runtime::spawn(async move {
            let interval = crate::acp::service::AcpService::IDLE_SWEEP_INTERVAL;
            let mut sweeps =
                tokio::time::interval_at(tokio::time::Instant::now() + interval, interval);
            sweeps.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            loop {
                sweeps.tick().await;
                let app = app.clone();
                let swept = tauri::async_runtime::spawn_blocking(move || {
                    let now = std::time::Instant::now();
                    app.state::<AppState>().acp.release_idle_runtimes(now)
                })
                .await;
                match swept {
                    Ok(Some(0)) => {}
                    Ok(Some(parked)) => tracing::info!(parked, "released idle Chats' runtimes"),
                    // A sweep that panicked leaves the next one to try again.
                    Err(_) => tracing::warn!("an idle-release sweep failed"),
                    Ok(None) => break,
                }
            }
        });
    }

    pub(crate) fn refresh_harnesses_after_window_shown(app: tauri::AppHandle) {
        tauri::async_runtime::spawn_blocking(move || {
            Self::rediscover_harnesses(&app);
        });
    }

    /// Discovers the installed harnesses again and re-admits them, so a
    /// harness installed or updated while Sage runs is picked up without a
    /// restart. Blocking: it runs each harness's bounded version probe. A
    /// due update check continues in the background.
    pub(crate) fn rediscover_harnesses(
        app: &tauri::AppHandle,
    ) -> Vec<crate::domain::HarnessDefinition> {
        let harnesses = &app.state::<AppState>().harnesses;
        let installed = {
            let _discovering = harnesses.lock_rediscovery();
            Self::discover_harnesses(app)
        };
        let sources = crate::harness_discovery::update_sources(&installed);
        let ids: Vec<_> = sources.iter().map(|source| source.harness_id).collect();
        if harnesses.begin_update_check(&ids) {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                use tauri::Emitter;
                let found = crate::harness_discovery::published_releases(&sources);
                let harnesses = &app.state::<AppState>().harnesses;
                harnesses.finish_update_check(found);
                let _ = app.emit("harness-discovery-changed-v1", harnesses.definitions());
            });
        }
        harnesses.definitions()
    }

    fn discover_harnesses(
        app: &tauri::AppHandle,
    ) -> Vec<crate::harness_discovery::InstalledHarness> {
        use tauri::Emitter;
        let state = app.state::<AppState>();
        crate::settings_commands::custom_acp_agents::apply_saved(&state);
        let presence = crate::harness_discovery::discover_installed_harness_presence();
        state.harnesses.apply_presence(presence.clone());
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
        state.harnesses.apply_discovery(claude, discovered.clone());
        let _ = app.emit(
            "harness-discovery-changed-v1",
            state.harnesses.definitions(),
        );
        discovered
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
                sage_core::error::AppError::InvalidRequest(
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
            archive_releases: Default::default(),
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
            std::sync::Arc::new(DesktopProjectionSink(app.clone(), Default::default())),
            registry.clone(),
        )
        .map_err(|_| {
            sage_core::error::AppError::InvalidRequest(
                "bounded ACP restart recovery failed; durable history was preserved".into(),
            )
        })?;
        #[cfg(windows)]
        if !fixture_isolation {
            acp.set_browser_app(app.clone());
        }
        let goal_app = app.clone();
        let commands_app = app.clone();
        acp.set_commands_notice_sink(std::sync::Arc::new(move |notice| {
            use tauri::Emitter;
            let _ = commands_app.emit("acp-commands-changed-v1", notice);
        }));
        acp.set_session_config_notice_sink(std::sync::Arc::new(move |notice| {
            use tauri::Emitter;
            let _ = app.emit("acp-session-config-changed-v1", notice);
        }));
        acp.set_goal_notice_sink(std::sync::Arc::new(move |notice| {
            use tauri::Emitter;
            let _ = goal_app.emit("acp-goal-changed-v1", notice);
        }));
        crate::chat_archive_commands::forget_read_marks_of_deleted_chats(&database, &settings);
        let harnesses = HarnessCatalog::with_registry(registry);
        if fixture_isolation {
            harnesses.apply_presence(Vec::new());
        }
        crate::settings_store::forget_removed_chat_access(&database, &settings);
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
            archive_releases: Default::default(),
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
            std::sync::Arc::new(DesktopProjectionSink(app.clone(), Default::default())),
            binding,
            registry.clone(),
        )
        .map_err(|_| {
            sage_core::error::AppError::InvalidRequest(
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
            archive_releases: Default::default(),
        })
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn try_new_characterization(
        database: Database,
        binding: crate::acp::service::CharacterizationBinding,
    ) -> AppResult<Self> {
        let settings = SettingsStore::in_memory()?;
        settings.editor_settings()?;
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let registry = std::sync::Arc::new(BindingRegistry::default());
        let acp =
            AcpService::new_characterization_for_test(database.clone(), binding, registry.clone())
                .map_err(|_| {
                    sage_core::error::AppError::InvalidRequest(
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
            archive_releases: Default::default(),
        })
    }

    #[cfg(test)]
    pub fn new(database: Database) -> Self {
        Self::try_new(database).expect("test ACP state must recover")
    }
}

struct DesktopProjectionSink(
    tauri::AppHandle,
    std::sync::Mutex<crate::acp::intent::ProjectionNoticeRoutes>,
);

impl NotificationSink for DesktopProjectionSink {
    fn observe_turn_notice(&self, event: &ProjectionChangedV1) {
        crate::notifications::observe(&self.0, event);
    }
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            use tauri::Emitter;
            let mut routes = self.1.lock().unwrap_or_else(|error| error.into_inner());
            match crate::acp::intent::route_projection_notice(&mut routes, event) {
                Ok(event) => self.0.emit(ACP_PROJECTION_CHANGED_EVENT, event),
                Err(child) => self.0.emit(
                    crate::acp::intent::ACP_CHILD_PROJECTION_CHANGED_EVENT,
                    child,
                ),
            }
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
