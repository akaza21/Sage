//! The session's mode, model and effort as ACP session config.
//!
//! Claude Code reports its models (with each model's effort levels) and the
//! starting permission mode in the `initialize` reply, and the effective model
//! and effort in `get_settings`. Sage's config overlay reads ACP `modes` and
//! `configOptions`, so this module presents those facts in that shape and
//! keeps the user's current selections.

use serde_json::{Value, json};

/// Permission modes Claude Code switches between with `set_permission_mode`
/// without extra launch flags.
const MODES: [(&str, &str, &str, &str); 4] = [
    (
        "default",
        "Manual",
        "Always ask before making changes",
        "standard",
    ),
    (
        "acceptEdits",
        "Accept edits",
        "Automatically accept all file edits",
        "standard",
    ),
    (
        "plan",
        "Plan",
        "Create a plan before making changes",
        "plan",
    ),
    (
        "auto",
        "Auto",
        "Claude handles permission decisions",
        "auto_review",
    ),
];

pub(super) const MODE_ID: &str = "mode";
pub(super) const MODEL_ID: &str = "model";
pub(super) const EFFORT_ID: &str = "effort";
const DEFAULT: &str = "default";

#[derive(Debug, Default)]
pub(super) struct SessionSettings {
    models: Vec<Value>,
    pub(super) mode: String,
    pub(super) model: String,
    pub(super) effort: String,
}

impl SessionSettings {
    /// Reads the `initialize` reply. The launch model, when Sage chose one,
    /// is the current model; otherwise Claude Code's default. A starting
    /// mode outside the picker is refused rather than shown as another.
    pub(super) fn from_initialize(reply: &Value, launch_model: Option<&str>) -> Option<Self> {
        let mut models = reply
            .get("models")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default();
        let model = launch_model.unwrap_or(DEFAULT);
        // A session launched with an alias Claude Code does not list still
        // runs that model, so the picker keeps it as the current choice.
        if !models
            .iter()
            .any(|entry| entry.get("value").and_then(Value::as_str) == Some(model))
        {
            models.push(json!({"value": model, "displayName": capitalized(model)}));
        }
        let mode = reply
            .get("current_permission_mode")
            .and_then(Value::as_str)
            .unwrap_or(DEFAULT);
        MODES.iter().any(|(id, ..)| *id == mode).then(|| Self {
            models,
            mode: mode.to_owned(),
            model: model.to_owned(),
            effort: DEFAULT.to_owned(),
        })
    }

    pub(super) fn has_mode(&self, mode: &str) -> bool {
        MODES.iter().any(|(id, ..)| *id == mode)
    }

    pub(super) fn has_model(&self, model: &str) -> bool {
        self.model_entry(model).is_some()
    }

    pub(super) fn has_effort(&self, effort: &str) -> bool {
        effort == DEFAULT || self.effort_levels().any(|level| level == effort)
    }

    /// Selecting a model resets effort when the new model lacks the level.
    pub(super) fn select_model(&mut self, model: &str) {
        self.model = model.to_owned();
        if !self.has_effort(&self.effort.clone()) {
            self.effort = DEFAULT.to_owned();
        }
    }

    /// The `effortLevel` flag setting for an effort selection; `default`
    /// clears the override.
    pub(super) fn effort_setting(effort: &str) -> Value {
        if effort == DEFAULT {
            Value::Null
        } else {
            effort.into()
        }
    }

    /// The ACP `session/new` result for `session_id`.
    pub(super) fn session_result(&self, session_id: &str) -> Value {
        json!({
            "sessionId": session_id,
            "modes": {
                "currentModeId": self.mode,
                "availableModes": MODES.map(|(id, name, description, kind)| json!({
                    "id": id, "name": name, "description": description, "_meta": {"kind": kind},
                })),
            },
            "configOptions": self.config_options(),
        })
    }

    pub(super) fn config_options(&self) -> Value {
        let models = self.models.iter().filter_map(|model| {
            Some(json!({
                "value": model.get("value")?.as_str()?,
                "name": model.get("displayName").and_then(Value::as_str)
                    .or_else(|| model.get("value").and_then(Value::as_str))?,
                "description": model.get("description").and_then(Value::as_str).unwrap_or_default(),
            }))
        });
        let efforts = std::iter::once(DEFAULT)
            .chain(self.effort_levels())
            .map(|level| json!({"value": level, "name": capitalized(level)}));
        let mut options = vec![
            json!({
                "id": MODE_ID, "name": "Mode", "description": "Session permission mode",
                "category": "mode", "type": "select", "currentValue": self.mode,
                "options": MODES.map(|(id, name, description, kind)| json!({
                    "value": id, "name": name, "description": description, "_meta": {"kind": kind},
                })),
            }),
            json!({
                "id": MODEL_ID, "name": "Model", "description": "AI model to use",
                "category": "model", "type": "select", "currentValue": self.model,
                "options": models.collect::<Vec<_>>(),
            }),
        ];
        if self.effort_levels().next().is_some() {
            options.push(json!({
                "id": EFFORT_ID, "name": "Effort",
                "description": "Available effort levels for this model",
                "category": "thought_level", "type": "select", "currentValue": self.effort,
                "options": efforts.collect::<Vec<_>>(),
            }));
        }
        options.into()
    }

    fn model_entry(&self, model: &str) -> Option<&Value> {
        self.models
            .iter()
            .find(|entry| entry.get("value").and_then(Value::as_str) == Some(model))
    }

    fn effort_levels(&self) -> impl Iterator<Item = &str> {
        self.model_entry(&self.model)
            .filter(|entry| entry.get("supportsEffort").and_then(Value::as_bool) == Some(true))
            .and_then(|entry| entry.get("supportedEffortLevels"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
    }
}

fn capitalized(value: &str) -> String {
    let mut chars = value.chars();
    chars.next().map_or_else(String::new, |first| {
        first.to_uppercase().chain(chars).collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settings() -> SessionSettings {
        SessionSettings::from_initialize(
            &json!({
                "current_permission_mode": "default",
                "models": [
                    {"value":"default","displayName":"Default (recommended)","description":"Sonnet","supportsEffort":true,"supportedEffortLevels":["low","high"]},
                    {"value":"haiku","displayName":"Haiku","description":"Fast"}
                ]
            }),
            None,
        )
        .unwrap()
    }

    #[test]
    fn session_result_offers_modes_models_and_the_current_models_effort_levels() {
        let result = settings().session_result("s-1");
        assert_eq!(result["sessionId"], "s-1");
        assert_eq!(result["modes"]["currentModeId"], "default");
        let options = result["configOptions"].as_array().unwrap();
        let ids: Vec<_> = options.iter().map(|o| o["id"].as_str().unwrap()).collect();
        assert_eq!(ids, ["mode", "model", "effort"]);
        let efforts: Vec<_> = options[2]["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| o["value"].as_str().unwrap())
            .collect();
        assert_eq!(efforts, ["default", "low", "high"]);
        assert_eq!(options[1]["options"][1]["name"], "Haiku");
    }

    #[test]
    fn a_model_without_effort_drops_the_effort_option_and_resets_the_level() {
        let mut settings = settings();
        settings.effort = "high".into();
        settings.select_model("haiku");
        assert_eq!(settings.effort, "default");
        let options = settings.config_options();
        assert_eq!(options.as_array().unwrap().len(), 2);
    }

    #[test]
    fn every_access_mode_selects_a_mode_this_binding_offers() {
        let offered = |mode: &str| settings().has_mode(mode);
        for mode in crate::acp::bindings::claude::CLAUDE_SDK_ACCESS_MODES {
            assert!(offered(mode.session_mode), "{}", mode.session_mode);
        }
        assert!(!offered("bypassPermissions"));
    }

    #[test]
    fn a_launch_alias_claude_code_does_not_list_stays_the_current_model() {
        let reply = json!({
            "current_permission_mode": "default",
            "models": [{"value":"default","displayName":"Default (recommended)"}],
        });
        let settings = SessionSettings::from_initialize(&reply, Some("sonnet")).unwrap();
        let options = settings.config_options();
        let model = &options[1];
        assert_eq!(model["currentValue"], "sonnet");
        let values: Vec<_> = model["options"]
            .as_array()
            .unwrap()
            .iter()
            .map(|o| (o["value"].as_str().unwrap(), o["name"].as_str().unwrap()))
            .collect();
        assert_eq!(
            values,
            [("default", "Default (recommended)"), ("sonnet", "Sonnet")]
        );
        assert!(settings.has_model("sonnet"));
    }

    #[test]
    fn a_starting_mode_outside_the_picker_is_refused() {
        let reply = |mode: &str| json!({"current_permission_mode": mode});
        assert!(SessionSettings::from_initialize(&reply("bypassPermissions"), None).is_none());
        let settings = SessionSettings::from_initialize(&reply("plan"), Some("opus")).unwrap();
        assert_eq!(
            (settings.mode.as_str(), settings.model.as_str()),
            ("plan", "opus")
        );
    }
}
