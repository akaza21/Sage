use super::*;

impl AppState {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub fn try_new_pi_fixture_desktop(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
        selection: crate::launch_profile::FixtureSelection,
    ) -> AppResult<Self> {
        let registry =
            std::sync::Arc::new(BindingRegistry::desktop_pi_fixture(selection).map_err(|_| {
                sage_core::error::AppError::InvalidRequest("Pi fixture admission failed".into())
            })?);
        Self::try_new_desktop_with_registry(database, settings, app, registry, true)
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub fn try_new_codex_fixture_desktop(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
        selection: crate::launch_profile::CodexFixtureSelection,
    ) -> AppResult<Self> {
        let registry = std::sync::Arc::new(
            BindingRegistry::desktop_codex_fixture(selection).map_err(|_| {
                sage_core::error::AppError::InvalidRequest(
                    "Codex fixture admission failed; durable history was preserved".into(),
                )
            })?,
        );
        Self::try_new_desktop_with_registry(database, settings, app, registry, true)
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub fn try_new_claude_fixture_desktop(
        database: Database,
        settings: SettingsStore,
        app: tauri::AppHandle,
        selection: crate::launch_profile::FixtureSelection,
    ) -> AppResult<Self> {
        let registry = std::sync::Arc::new(
            BindingRegistry::desktop_claude_fixture(selection).map_err(|_| {
                sage_core::error::AppError::InvalidRequest("Claude fixture admission failed".into())
            })?,
        );
        Self::try_new_desktop_with_registry(database, settings, app, registry, true)
    }
}
