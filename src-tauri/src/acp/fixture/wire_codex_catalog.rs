use serde_json::{Value, json};

use super::WireState;

const CONFIG_MODELS: [&str; 2] = ["fixture-model", "fixture-model-deep"];

/// A restart journey's thread runs the model it was opened or resumed with,
/// at the catalogue's default effort, as Codex reports it.
pub(super) fn config_thread(state: &WireState, thread_id: &str) -> Value {
    json!({
        "thread":{"id": thread_id},
        "model": state.codex_thread_model.as_deref()
            .filter(|model| CONFIG_MODELS.contains(model)).unwrap_or(CONFIG_MODELS[0]),
        "reasoningEffort":"medium"
    })
}

pub(super) fn models(shape: &str) -> Value {
    let mut catalog = json!({"data":[{
        "id":"fixture-model","model":"fixture-model","displayName":"Synthetic replay",
        "description":"Synthetic native replay model","isDefault":true,"hidden":false,
        "supportedReasoningEfforts":[]
    }],"nextCursor":null});
    if shape != "models" {
        catalog["data"][0]["defaultReasoningEffort"] = json!("medium");
        catalog["data"][0]["supportedReasoningEfforts"] = json!([
            {"reasoningEffort":"low","description":"Quick synthetic replay"},
            {"reasoningEffort":"medium","description":"Balanced synthetic replay"},
            {"reasoningEffort":"high","description":"Thorough synthetic replay"}
        ]);
        if shape == "configModels" {
            // Every field a pinned Codex `model/list` entry carries, as
            // account discovery requires.
            let full = json!({"upgrade":null,"upgradeInfo":null,"availabilityNux":null,"inputModalities":["text"],"supportsPersonality":false,"additionalSpeedTiers":[],"serviceTiers":[],"defaultServiceTier":null,"modelSpecialty":null,"multiAgentVersion":null,"availableAccessPrograms":null});
            catalog["data"][0]
                .as_object_mut()
                .unwrap()
                .extend(full.as_object().unwrap().clone());
        }
        let mut alternate = catalog["data"][0].clone();
        alternate["id"] = json!("fixture-model-fast");
        alternate["model"] = json!("fixture-model-fast");
        alternate["displayName"] = json!("Synthetic replay fast");
        alternate["isDefault"] = json!(false);
        alternate["defaultReasoningEffort"] = json!("low");
        if shape == "configModels" {
            alternate["id"] = json!(CONFIG_MODELS[1]);
            alternate["model"] = json!(CONFIG_MODELS[1]);
            alternate["displayName"] = json!("Synthetic replay deep");
            alternate["defaultReasoningEffort"] = json!("medium");
        }
        catalog["data"].as_array_mut().unwrap().push(alternate);
    }
    catalog
}

/// The model and effort a `turn/start` named. Codex runs the thread's own
/// for any it leaves out.
pub(super) fn turn_config(request: &Value) -> String {
    let named = |key: &str| {
        request
            .get("params")
            .and_then(|params| params.get(key))
            .and_then(Value::as_str)
            .unwrap_or("none, so the thread's own")
    };
    format!(
        "This turn/start named model: {}; effort: {}.",
        named("model"),
        named("effort")
    )
}
