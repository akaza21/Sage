//! Settings commands for user-defined ACP agents. Saving runs the same
//! executable check admission runs before every launch and refuses with its
//! reason before anything is written; then the registrations are made to
//! hold the saved list and the harness catalog is republished, so the agent
//! appears in the harness picker at once. Hashing runs off the main thread.

use tauri::{AppHandle, Emitter, Manager, State};

use crate::{
    acp::bindings::standard_acp::custom_executable::check_custom_executable,
    settings_store::{CustomAcpAgent, CustomAcpAgentDraft},
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
    let (_, executable, _, _) = crate::settings_store::validate_custom_acp_agent(draft)
        .map_err(|error| error.to_string())?;
    check_custom_executable(std::path::Path::new(&executable))
        .map_err(|refusal| refusal.message())?;
    state
        .settings
        .save_custom_acp_agent(draft)
        .map_err(|error| error.to_string())?;
    republish(state)
}

pub(crate) fn remove(state: &AppState, id: &str) -> Result<Vec<CustomAcpAgent>, String> {
    state
        .settings
        .remove_custom_acp_agent(id)
        .map_err(|error| error.to_string())?;
    republish(state)
}

/// Makes the registrations hold the saved list and returns it.
fn republish(state: &AppState) -> Result<Vec<CustomAcpAgent>, String> {
    let agents = state
        .settings
        .custom_acp_agents()
        .map_err(|error| error.to_string())?;
    state
        .acp
        .binding_registry()
        .apply_custom_acp_agents(&agents);
    Ok(agents)
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
    use crate::{
        acp::bindings::standard_acp::custom_executable::write_test_executable,
        persistence::Database, settings_store::CustomAcpAgentDraft,
    };

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
        write_test_executable(&executable, b"agent");
        let state = AppState::new(Database::open(std::path::Path::new(":memory:")).unwrap());
        assert_eq!(
            save(&state, &draft(&root.join("missing.exe"))).unwrap_err(),
            "The executable does not exist."
        );
        assert!(
            save(&state, &draft(&root)).is_err(),
            "a directory is refused"
        );
        let saved = save(&state, &draft(&executable)).unwrap();
        assert_eq!(saved.len(), 1);
        let registry = state.acp.binding_registry();
        assert!(
            registry
                .custom_acp("custom-acp-1")
                .unwrap()
                .admitted()
                .is_some()
        );
        assert!(registry.resolve("custom-acp-1").is_ok());
        assert!(remove(&state, "custom-acp-1").unwrap().is_empty());
        assert!(registry.resolve("custom-acp-1").is_err());
        let again = save(&state, &draft(&executable)).unwrap();
        assert_eq!(again[0].id, "custom-acp-2", "a removed id is not reused");
        assert!(registry.resolve("custom-acp-1").is_err());
        assert!(registry.resolve("custom-acp-2").is_ok());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn a_link_is_refused_at_save_with_its_target_before_anything_is_written() {
        let root = crate::test_paths::temp_dir()
            .join(format!("sage-custom-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let target = root.join("agent");
        write_test_executable(&target, b"agent");
        let link = root.join("agent-link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let state = AppState::new(Database::open(std::path::Path::new(":memory:")).unwrap());
        let refusal = save(&state, &draft(&link)).unwrap_err();
        assert_eq!(
            refusal,
            format!(
                "The executable is a link. Sage launches only the file itself: use {} instead.",
                target.display()
            )
        );
        assert!(state.settings.custom_acp_agents().unwrap().is_empty());
        let unexecutable = root.join("plain");
        std::fs::write(&unexecutable, b"agent").unwrap();
        assert!(save(&state, &draft(&unexecutable)).is_err());
        assert!(state.settings.custom_acp_agents().unwrap().is_empty());
        std::fs::remove_dir_all(root).unwrap();
    }
}
