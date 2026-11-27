pub(crate) mod acp;

use std::time::Duration;

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

#[tauri::command]
pub async fn refresh_harnesses(app: tauri::AppHandle) -> Result<Vec<HarnessDefinition>, String> {
    tauri::async_runtime::spawn_blocking(move || AppState::rediscover_harnesses(&app))
        .await
        .map_err(|error| error.to_string())
}

/// The curated built-in commands the new-chat draft offers for a harness.
/// Metadata only: no harness process starts.
#[tauri::command]
pub fn list_builtin_commands(state: State<'_, AppState>, harness_id: String) -> Vec<String> {
    state.harnesses.draft_builtin_commands(&harness_id)
}

/// A model probe can also learn the start mode the harness's own settings name, which moves its
/// default access; the catalog is sent again when that changes what the WebView was told.
///
/// A live listing is kept, so the next start shows it before asking again.
#[tauri::command]
pub async fn list_harness_models(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    harness_id: String,
) -> Result<Vec<ModelOption>, String> {
    let access = |harnesses: &HarnessCatalog| {
        let definition = harnesses
            .definitions()
            .into_iter()
            .find(|d| d.id == harness_id);
        definition.and_then(|definition| serde_json::to_value(definition.access_modes).ok())
    };
    let before = access(&state.harnesses);
    let models =
        list_harness_models_async(&state.harnesses, &harness_id, MODEL_LISTING_DEADLINE).await;
    if let Ok(models) = &models {
        let _ = state.settings.remember_harness_models(
            &harness_id,
            models,
            sage_core::time::now_millis(),
        );
    }
    if access(&state.harnesses) != before {
        use tauri::Emitter;
        let _ = app.emit(
            "harness-discovery-changed-v1",
            state.harnesses.definitions(),
        );
    }
    models
}

trait HarnessModelLookup: Clone + Send + Sync + 'static {
    fn list_models(&self, harness_id: &str) -> Result<Vec<ModelOption>, String>;
    /// The name the WebView shows for the harness, for an error about it.
    fn harness_name(&self, harness_id: &str) -> String;
}

impl HarnessModelLookup for HarnessCatalog {
    fn list_models(&self, harness_id: &str) -> Result<Vec<ModelOption>, String> {
        list_harness_models_inner(self, harness_id)
    }

    fn harness_name(&self, harness_id: &str) -> String {
        self.definitions()
            .into_iter()
            .find(|definition| definition.id == harness_id)
            .map_or_else(|| harness_id.to_owned(), |definition| definition.name)
    }
}

/// Above a probe's own bound plus one probe queued ahead of it. A lookup
/// that outlasts it still ends in an error the composer offers Retry on,
/// never in a listing that stays "Loading…".
const MODEL_LISTING_DEADLINE: Duration = Duration::from_secs(45);

async fn list_harness_models_async(
    lookup: &impl HarnessModelLookup,
    harness_id: &str,
    deadline: Duration,
) -> Result<Vec<ModelOption>, String> {
    let name = lookup.harness_name(harness_id);
    let lookup = lookup.clone();
    let harness_id = harness_id.to_owned();
    let listing = tauri::async_runtime::spawn_blocking(move || lookup.list_models(&harness_id));
    match tokio::time::timeout(deadline, listing).await {
        Ok(joined) => joined.map_err(|error| error.to_string())?,
        Err(_) => Err(models_unlisted_message(&name)),
    }
}

fn list_harness_models_inner(
    harnesses: &HarnessCatalog,
    harness_id: &str,
) -> Result<Vec<ModelOption>, String> {
    if harness_id == "codex" {
        let _ = harnesses.refresh_codex_registration();
    }
    let models = harnesses
        .models(harness_id)
        .map_err(|error| error.to_string())?;
    match harnesses.models_unlisted(harness_id) {
        Some(name) if models.is_empty() => Err(models_unlisted_message(&name)),
        _ => Ok(models),
    }
}

/// A build that should list models and did not answer in time; the first
/// start after an update can take that long, so a retry usually lists them.
fn models_unlisted_message(name: &str) -> String {
    format!("{name} did not list its models in time. Retry in a moment.")
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
        let actual = list_harness_models_async(
            &HarnessCatalog::default(),
            "sage-simulator",
            MODEL_LISTING_DEADLINE,
        )
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
            list_harness_models_async(&harnesses, "missing-harness", MODEL_LISTING_DEADLINE)
                .await
                .unwrap_err(),
            "unknown harness: missing-harness"
        );
        assert_eq!(
            serde_json::to_value(
                list_harness_models_async(&harnesses, "codex", MODEL_LISTING_DEADLINE)
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
            list_harness_models_inner(&self.harnesses, harness_id)
        }

        fn harness_name(&self, harness_id: &str) -> String {
            self.harnesses.harness_name(harness_id)
        }
    }

    #[tokio::test]
    async fn a_lookup_that_never_answers_ends_in_an_error_naming_the_harness() {
        let (entered_tx, _entered_rx) = mpsc::sync_channel(1);
        let (release_tx, release_rx) = mpsc::sync_channel(1);
        let lookup = HeldLookup {
            harnesses: HarnessCatalog::default(),
            entered: entered_tx,
            release: Arc::new(Mutex::new(release_rx)),
        };
        let started = std::time::Instant::now();
        let listed =
            list_harness_models_async(&lookup, "sage-simulator", Duration::from_millis(200)).await;
        let _ = release_tx.send(());
        let name = lookup.harness_name("sage-simulator");
        assert_eq!(listed.unwrap_err(), models_unlisted_message(&name));
        assert!(started.elapsed() < Duration::from_secs(5));
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
                    let models = list_harness_models_async(
                        &lookup,
                        "sage-simulator",
                        MODEL_LISTING_DEADLINE,
                    )
                    .await;
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
