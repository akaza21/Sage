//! Settings commands for user-defined ACP agents. Saving proves the
//! executable is an existing regular file, admits it into its fixed slot
//! and republishes the harness catalog, so the agent appears in the harness
//! picker at once. Hashing the executable runs off the main thread.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    settings_store::{CustomAcpAgent, CustomAcpAgentDraft, custom_acp_slot},
    state::AppState,
};

#[tauri::command]
pub(crate) fn custom_acp_agents(state: State<'_, AppState>) -> Result<Vec<CustomAcpAgent>, String> {
    state
        .settings
        .custom_acp_agents()
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub(crate) async fn save_custom_acp_agent(
    app: AppHandle,
    agent: CustomAcpAgentDraft,
) -> Result<Vec<CustomAcpAgent>, String> {
    blocking(app, move |state| save(state, &agent)).await
}

#[tauri::command]
pub(crate) async fn remove_custom_acp_agent(
    app: AppHandle,
    id: String,
) -> Result<Vec<CustomAcpAgent>, String> {
    blocking(app, move |state| remove(state, &id)).await
}

async fn blocking(
    app: AppHandle,
    work: impl FnOnce(&AppState) -> Result<Vec<CustomAcpAgent>, String> + Send + 'static,
) -> Result<Vec<CustomAcpAgent>, String> {
    let handle = app.clone();
    let agents = tauri::async_runtime::spawn_blocking(move || work(&handle.state::<AppState>()))
        .await
        .map_err(|_| "Saving the agent stopped unexpectedly.".to_owned())??;
    let _ = app.emit(
        "harness-discovery-changed-v1",
        app.state::<AppState>().harnesses.definitions(),
    );
    Ok(agents)
}

pub(crate) fn save(
    state: &AppState,
    draft: &CustomAcpAgentDraft,
) -> Result<Vec<CustomAcpAgent>, String> {
    crate::settings_store::validate_custom_acp_agent(draft).map_err(|error| error.to_string())?;
    if !std::fs::metadata(draft.executable.trim()).is_ok_and(|metadata| metadata.is_file()) {
        return Err("The executable does not exist or is not a file.".into());
    }
    let saved = state
        .settings
        .save_custom_acp_agent(draft)
        .map_err(|error| error.to_string())?;
    if let Some(registration) =
        custom_acp_slot(&saved.id).and_then(|slot| state.acp.binding_registry().custom_acp(slot))
    {
        registration.apply(Some(&saved));
    }
    state
        .settings
        .custom_acp_agents()
        .map_err(|error| error.to_string())
}

pub(crate) fn remove(state: &AppState, id: &str) -> Result<Vec<CustomAcpAgent>, String> {
    let slot = state
        .settings
        .remove_custom_acp_agent(id)
        .map_err(|error| error.to_string())?;
    if let Some(registration) = state.acp.binding_registry().custom_acp(slot) {
        registration.apply(None);
    }
    state
        .settings
        .custom_acp_agents()
        .map_err(|error| error.to_string())
}

/// Admits the saved agents at startup, off the main thread.
pub(crate) fn apply_saved(state: &AppState) {
    match state.settings.custom_acp_agents() {
        Ok(agents) => state
            .acp
            .binding_registry()
            .apply_custom_acp_agents(&agents),
        Err(error) => tracing::warn!(%error, "custom ACP agents could not be read"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{persistence::Database, settings_store::CustomAcpAgentDraft};

    fn draft(executable: &std::path::Path) -> CustomAcpAgentDraft {
        CustomAcpAgentDraft {
            id: None,
            name: "Team agent".into(),
            executable: executable.to_string_lossy().into_owned(),
            args: vec!["--acp".into()],
            env: Vec::new(),
        }
    }

    #[test]
    fn saving_admits_the_agent_and_removing_clears_its_harness() {
        let root = crate::test_paths::temp_dir()
            .join(format!("sage-custom-command-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let executable = root.join("agent.exe");
        std::fs::write(&executable, b"agent").unwrap();
        let state = AppState::new(Database::open(std::path::Path::new(":memory:")).unwrap());
        assert!(save(&state, &draft(&root.join("missing.exe"))).is_err());
        assert!(
            save(&state, &draft(&root)).is_err(),
            "a directory is refused"
        );
        let saved = save(&state, &draft(&executable)).unwrap();
        assert_eq!(saved.len(), 1);
        let registry = state.acp.binding_registry();
        assert!(registry.custom_acp(1).unwrap().admitted().is_some());
        assert!(registry.resolve("custom-acp-1").is_ok());
        assert!(remove(&state, "custom-acp-1").unwrap().is_empty());
        assert!(registry.resolve("custom-acp-1").is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
