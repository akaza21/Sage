use crate::{
    acp::{
        bindings::codex::startup_codex_binding_registration,
        contract::{ProjectGeneration, SageProjectId},
        intent::{ACP_PROJECTION_CHANGED_EVENT, ProjectionChangedV1},
        runtime::{NotificationError, NotificationSink, NotificationSinkFuture},
        service::AcpService,
    },
    editor_coordination::EditorOperationCoordinator,
    error::AppResult,
    harnesses::HarnessCatalog,
    persistence::Database,
};

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
    pub acp: AcpService,
    pub harnesses: HarnessCatalog,
    pub editor_operations: EditorOperationCoordinator,
    pub project_transitions: tokio::sync::Mutex<u64>,
    pub startup_recovery_notice: Option<String>,
}

impl AppState {
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
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let codex = startup_codex_binding_registration();
        let acp =
            AcpService::new_with_codex_for_test(database.clone(), codex.clone()).map_err(|_| {
                crate::error::AppError::InvalidRequest(
                    "bounded ACP restart recovery failed; durable history was preserved".into(),
                )
            })?;
        Ok(Self {
            database,
            acp,
            harnesses: HarnessCatalog::with_codex_registration(codex),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
        })
    }

    pub fn try_new_desktop(database: Database, app: tauri::AppHandle) -> AppResult<Self> {
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let codex = startup_codex_binding_registration();
        let acp = AcpService::new_with_notification_sink_and_codex(
            database.clone(),
            std::sync::Arc::new(DesktopProjectionSink(app)),
            codex.clone(),
        )
        .map_err(|_| {
            crate::error::AppError::InvalidRequest(
                "bounded ACP restart recovery failed; durable history was preserved".into(),
            )
        })?;
        Ok(Self {
            database,
            acp,
            harnesses: HarnessCatalog::with_codex_registration(codex),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
        })
    }

    #[cfg(all(windows, feature = "acp-fixture"))]
    pub fn try_new_characterization_desktop(
        database: Database,
        app: tauri::AppHandle,
        binding: crate::acp::service::CharacterizationBinding,
    ) -> AppResult<Self> {
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let codex = startup_codex_binding_registration();
        let acp = AcpService::new_with_characterization_sink(
            database.clone(),
            std::sync::Arc::new(DesktopProjectionSink(app)),
            binding,
            codex.clone(),
        )
        .map_err(|_| {
            crate::error::AppError::InvalidRequest(
                "bounded ACP restart recovery failed; durable history was preserved".into(),
            )
        })?;
        Ok(Self {
            database,
            acp,
            harnesses: HarnessCatalog::with_codex_registration_and_characterization(codex),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
        })
    }

    #[cfg(all(test, windows, feature = "acp-fixture"))]
    pub(crate) fn try_new_characterization(
        database: Database,
        binding: crate::acp::service::CharacterizationBinding,
    ) -> AppResult<Self> {
        let trash_recovery = crate::resource_trash_commands::recover_pending_at_startup(&database)?;
        let transition_epoch = trash_recovery
            .as_ref()
            .map_or(0, |recovery| recovery.transition_epoch);
        let codex = startup_codex_binding_registration();
        let acp =
            AcpService::new_characterization_for_test(database.clone(), binding, codex.clone())
                .map_err(|_| {
                    crate::error::AppError::InvalidRequest(
                        "bounded ACP restart recovery failed; durable history was preserved".into(),
                    )
                })?;
        Ok(Self {
            database,
            acp,
            harnesses: HarnessCatalog::with_codex_registration_and_characterization(codex),
            editor_operations: EditorOperationCoordinator::default(),
            project_transitions: tokio::sync::Mutex::new(transition_epoch),
            startup_recovery_notice: trash_recovery.map(|recovery| recovery.notice),
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
        assert!(
            state
                .acp
                .codex_registration()
                .same_authority(&state.harnesses.codex_registration())
        );
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
