use super::AppState;
use tauri::Manager;

impl AppState {
    pub(crate) fn refresh_harnesses_after_window_shown(app: tauri::AppHandle) {
        app.state::<AppState>()
            .database
            .attention
            .attach(app.clone());
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
}
