mod harness_refresh;

use sage_core::error::AppResult;

use crate::project_registry::ProjectRegistryStore;
use crate::{
    acp::{
        bindings::registry::BindingRegistry,
        ipc::{ACP_PROJECTION_CHANGED_EVENT, ProjectionChangedV1},
        runtime::{NotificationError, NotificationSink, NotificationSinkFuture},
        service::AcpService,
    },
    editor_coordination::EditorOperationCoordinator,
    harnesses::HarnessCatalog,
    settings_store::SettingsStore,
};
use sage_acp_protocol::contract::{ProjectGeneration, SageProjectId};
use sage_db::database::Database;
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
        let access_app = app.clone();
        acp.set_harness_turn_access(std::sync::Arc::new(move |owner| {
            use tauri::Manager;
            let state = access_app.try_state::<AppState>()?;
            crate::commands::acp::session_access::harness_turn_grant(&state, owner)
        }));
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
    std::sync::Mutex<crate::acp::ipc::ProjectionNoticeRoutes>,
);

impl NotificationSink for DesktopProjectionSink {
    fn observe_turn_notice(&self, event: &ProjectionChangedV1) {
        crate::notifications::observe(&self.0, event);
    }
    fn publish<'a>(&'a self, event: ProjectionChangedV1) -> NotificationSinkFuture<'a> {
        Box::pin(async move {
            use tauri::Emitter;
            let mut routes = self.1.lock().unwrap_or_else(|error| error.into_inner());
            match crate::acp::ipc::route_projection_notice(&mut routes, event) {
                Ok(event) => self.0.emit(ACP_PROJECTION_CHANGED_EVENT, event),
                Err(child) => self
                    .0
                    .emit(crate::acp::ipc::ACP_CHILD_PROJECTION_CHANGED_EVENT, child),
            }
            .map_err(|_| NotificationError::SinkUnavailable)
        })
    }
}

#[cfg(test)]
mod tests;

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
#[path = "state_fixture.rs"]
mod desktop_fixture;
