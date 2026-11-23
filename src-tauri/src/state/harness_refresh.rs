use super::AppState;
use tauri::Manager;

impl AppState {
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
    ) -> Vec<sage_domain::harness::HarnessDefinition> {
        let harnesses = &app.state::<AppState>().harnesses;
        let installed = {
            let _discovering = harnesses.lock_rediscovery();
            Self::discover_harnesses(app)
        };
        let sources = crate::harness_discovery::update_sources(&installed);
        let ids: Vec<_> = sources.iter().map(|source| source.harness_id).collect();
        if let Some(epoch) = Self::npm_check_enabled(app)
            .then(|| harnesses.begin_update_check(&ids))
            .flatten()
        {
            let app = app.clone();
            tauri::async_runtime::spawn_blocking(move || {
                use tauri::Emitter;
                // The switch is read again before each npm lookup, and a check
                // begun before it turned off is dropped by its epoch.
                let harnesses = &app.state::<AppState>().harnesses;
                let found = crate::harness_discovery::published_releases(
                    &sources,
                    &harnesses.update_check_live(epoch, || Self::npm_check_enabled(&app)),
                );
                harnesses.finish_update_check(epoch, found);
                let _ = app.emit("harness-discovery-changed-v1", harnesses.definitions());
            });
        }
        harnesses.definitions()
    }

    /// An unreadable setting counts as off: no npm process on doubt.
    pub(crate) fn npm_check_enabled(app: &tauri::AppHandle) -> bool {
        app.state::<AppState>()
            .settings
            .harness_update_check()
            .unwrap_or(false)
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
}
