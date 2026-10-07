pub(crate) mod acp;

use tauri::State;

use crate::{
    domain::{HarnessDefinition, ModelOption, SimulatorTurnResult},
    harnesses::{SIMULATOR_DEFAULT_MODEL_ID, SIMULATOR_ID},
    state::AppState,
};

const MAX_PROMPT_BYTES: usize = 100_000;

#[tauri::command]
pub fn list_harnesses(state: State<'_, AppState>) -> Vec<HarnessDefinition> {
    state.harnesses.definitions()
}

#[tauri::command]
pub fn list_harness_models(
    state: State<'_, AppState>,
    harness_id: String,
) -> Result<Vec<ModelOption>, String> {
    if harness_id == "codex" {
        let _ = state.harnesses.refresh_codex_registration();
    }
    state
        .harnesses
        .models(&harness_id)
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn send_simulated_prompt(
    state: State<'_, AppState>,
    chat_id: String,
    harness_id: String,
    model_id: Option<String>,
    text: String,
) -> Result<SimulatorTurnResult, String> {
    let text = text.trim();
    if text.is_empty() {
        return Err("prompt must not be empty".into());
    }
    if text.len() > MAX_PROMPT_BYTES {
        return Err(format!(
            "prompt exceeds the {MAX_PROMPT_BYTES}-byte Phase 0 limit"
        ));
    }
    if harness_id != SIMULATOR_ID {
        return Err("install and initialize this harness before sending a prompt".into());
    }
    if !crate::harnesses::SIMULATOR_IN_CATALOG {
        return Err("the development simulator is not part of this build".into());
    }
    let advertised_models = state
        .harnesses
        .models(&harness_id)
        .map_err(|error| error.to_string())?;
    if let Some(model_id) = &model_id
        && !advertised_models.iter().any(|model| &model.id == model_id)
    {
        return Err("selected model is not advertised by this harness".into());
    }
    let model_id = model_id.as_deref().unwrap_or(SIMULATOR_DEFAULT_MODEL_ID);
    let response = format!(
        "Simulator acknowledged the prompt using {model_id}. The ACP supervisor remains a later phase."
    );
    let _transition = state.project_transitions.lock().await;
    state
        .database
        .append_simulator_turn_for_model(&chat_id, model_id, text, &response)
        .map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests;
