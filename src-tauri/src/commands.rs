pub(crate) mod acp;

use tauri::State;

use crate::session_events::SessionEventStore;
use crate::{
    harnesses::{HarnessCatalog, SIMULATOR_DEFAULT_MODEL_ID, SIMULATOR_ID},
    state::AppState,
};
use sage_domain::{
    chat::SimulatorTurnResult,
    harness::{HarnessDefinition, ModelOption},
};

const MAX_PROMPT_BYTES: usize = 100_000;

#[tauri::command]
pub fn list_harnesses(state: State<'_, AppState>) -> Vec<HarnessDefinition> {
    state.harnesses.definitions()
}

/// Discovers the installed harnesses again, for Settings' Refresh discovery.
#[tauri::command]
pub async fn refresh_harnesses(app: tauri::AppHandle) -> Result<Vec<HarnessDefinition>, String> {
    tauri::async_runtime::spawn_blocking(move || AppState::rediscover_harnesses(&app))
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_harness_models(
    state: State<'_, AppState>,
    harness_id: String,
) -> Result<Vec<ModelOption>, String> {
    list_harness_models_async(&state.harnesses, &harness_id).await
}

trait HarnessModelLookup: Clone + Send + Sync + 'static {
    fn list_models(&self, harness_id: &str) -> Result<Vec<ModelOption>, String>;
}

impl HarnessModelLookup for HarnessCatalog {
    fn list_models(&self, harness_id: &str) -> Result<Vec<ModelOption>, String> {
        list_harness_models_inner(self, harness_id)
    }
}

async fn list_harness_models_async(
    lookup: &impl HarnessModelLookup,
    harness_id: &str,
) -> Result<Vec<ModelOption>, String> {
    let lookup = lookup.clone();
    let harness_id = harness_id.to_owned();
    tauri::async_runtime::spawn_blocking(move || lookup.list_models(&harness_id))
        .await
        .map_err(|error| error.to_string())?
}

fn list_harness_models_inner(
    harnesses: &HarnessCatalog,
    harness_id: &str,
) -> Result<Vec<ModelOption>, String> {
    if harness_id == "codex" {
        let _ = harnesses.refresh_codex_registration();
    }
    harnesses
        .models(harness_id)
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

#[cfg(test)]
mod model_lookup_tests {
    use super::*;
    use std::{
        sync::{Arc, Mutex, mpsc},
        time::Duration,
    };

    fn expected_simulator_models() -> serde_json::Value {
        serde_json::json!([
            {
                "id": "sim-fast",
                "name": "Simulator Fast",
                "description": "Short deterministic responses for the walking skeleton.",
                "isDefault": true
            },
            {
                "id": "sim-deep",
                "name": "Simulator Deep",
                "description": "A second option for testing model changes.",
                "isDefault": false
            }
        ])
    }

    #[tokio::test]
    async fn model_lookup_returns_the_two_simulator_models() {
        let actual = list_harness_models_async(&HarnessCatalog::default(), "sage-simulator")
            .await
            .unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            expected_simulator_models()
        );
    }

    #[tokio::test]
    async fn model_lookup_distinguishes_unknown_harness_from_no_account_models() {
        let harnesses = HarnessCatalog::default();
        assert_eq!(
            list_harness_models_async(&harnesses, "missing-harness")
                .await
                .unwrap_err(),
            "unknown harness: missing-harness"
        );
        assert_eq!(
            serde_json::to_value(
                list_harness_models_async(&harnesses, "codex")
                    .await
                    .unwrap()
            )
            .unwrap(),
            serde_json::json!([])
        );
    }

    #[derive(Clone)]
    struct HeldLookup {
        harnesses: HarnessCatalog,
        entered: mpsc::SyncSender<()>,
        release: Arc<Mutex<mpsc::Receiver<()>>>,
    }

    impl HarnessModelLookup for HeldLookup {
        fn list_models(&self, harness_id: &str) -> Result<Vec<ModelOption>, String> {
            self.entered.send(()).unwrap();
            self.release
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(15))
                .unwrap();
            // Delay the real lookup, without substituting its output.
            list_harness_models_inner(&self.harnesses, harness_id)
        }
    }

    #[test]
    fn blocked_model_lookup_leaves_the_async_runtime_responsive() {
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let (start_tx, start_rx) = tokio::sync::oneshot::channel();
        let (progress_tx, progress_rx) = mpsc::sync_channel(1);
        let lookup = HeldLookup {
            harnesses: HarnessCatalog::default(),
            entered: entered_tx,
            release: Arc::new(Mutex::new(release_rx)),
        };
        let runtime_thread = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let unrelated = tokio::spawn(async move {
                        start_rx.await.unwrap();
                        tokio::task::yield_now().await;
                        progress_tx.send(()).unwrap();
                    });
                    let models = list_harness_models_async(&lookup, "sage-simulator").await;
                    unrelated.await.unwrap();
                    models
                })
        });
        // Only permit unrelated work after the lookup has entered its blocking wait.
        let entered = entered_rx.recv_timeout(Duration::from_secs(5));
        let _ = start_tx.send(());
        let progress = progress_rx.recv_timeout(Duration::from_secs(5));
        // Release and join even on failure, so a synchronous regression cannot hang the suite.
        let _ = release_tx.send(());
        let models = runtime_thread.join().unwrap().unwrap();
        assert!(entered.is_ok(), "lookup never reached its blocking wait");
        assert!(
            progress.is_ok(),
            "unrelated async work stalled behind the lookup"
        );
        assert_eq!(
            serde_json::to_value(models).unwrap(),
            expected_simulator_models()
        );
    }
}
