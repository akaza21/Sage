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
        let last_used = Self::last_used_harness(&state);
        let (claude, discovered) = std::thread::scope(|scope| {
            let claude = scope.spawn(crate::harness_discovery::discover_claude_launch);
            // Each harness is admitted and shown as soon as its own probe
            // answers; a slow one never holds back the rest.
            let discovered = crate::harness_discovery::enrich_installed_harness_versions_each(
                &presence,
                &state.harness_version_cache,
                |probed| {
                    state.harnesses.apply_probed(probed);
                    let _ = app.emit(
                        "harness-discovery-changed-v1",
                        state.harnesses.definitions(),
                    );
                    if last_used.as_deref() == Some(probed.id) {
                        Self::prefetch_models(app, probed.id);
                    }
                },
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

    /// The harness of the latest session, when it lists its models through
    /// the generic listing (an account-model harness lists through its own
    /// discovery) and discovery has a curated or admitted binding for it.
    fn last_used_harness(state: &AppState) -> Option<String> {
        let (binding_id, _) = crate::workbench_persistence::store::latest_session_binding(
            &state.database.connection(),
        )
        .ok()
        .flatten()?;
        state.harnesses.harness_serving_binding(&binding_id)
    }

    /// Starts the live listing of the harness used last as soon as it is
    /// admitted, off the startup path; the composer's own request then shares
    /// it or finds it kept. Bounded by the listing's own deadline.
    fn prefetch_models(app: &tauri::AppHandle, harness_id: &str) {
        if !lists_generically(&app.state::<AppState>().harnesses.definitions(), harness_id) {
            return;
        }
        let app = app.clone();
        let harness_id = harness_id.to_owned();
        tauri::async_runtime::spawn(async move {
            let state = app.state::<AppState>();
            let _ = crate::commands::list_and_remember(&app, &state, &harness_id).await;
        });
    }
}

/// Whether `harness_id` lists its models through the generic listing the
/// composer asks for; an account-model harness lists through its own
/// discovery, which a prefetch would start for nothing.
fn lists_generically(
    definitions: &[sage_domain::harness::HarnessDefinition],
    harness_id: &str,
) -> bool {
    definitions
        .iter()
        .any(|definition| definition.id == harness_id && !definition.account_model_discovery)
}

#[cfg(test)]
mod prefetch_tests {
    use super::lists_generically;
    use crate::harnesses::HarnessCatalog;

    #[test]
    fn only_a_harness_listed_generically_is_prefetched() {
        let mut definitions = HarnessCatalog::default().definitions();
        let pi = definitions.iter_mut().find(|h| h.id == "pi").unwrap();
        pi.account_model_discovery = false;
        assert!(lists_generically(&definitions, "pi"));
        let pi = definitions.iter_mut().find(|h| h.id == "pi").unwrap();
        pi.account_model_discovery = true;
        assert!(!lists_generically(&definitions, "pi"));
        assert!(!lists_generically(&definitions, "not-a-harness"));
    }
}
