//! The Claude ACP fixture's session config: model, effort and the permission
//! mode, which claude-agent-acp 0.76.0 keeps as config options.

use super::SESSION;
use serde_json::{Value, json};

/// The session config of a journey that changes model, effort and mode: it
/// answers each change with the new value, as claude-agent-acp does, and a
/// new or resumed session starts again from the adapter's defaults.
pub(crate) struct AcpConfig {
    model: &'static str,
    effort: &'static str,
    mode: &'static str,
}

pub(super) const MODELS: [&str; 2] = ["fixture-model", "fixture-model-deep"];
const EFFORTS: [&str; 3] = ["low", "medium", "high"];
// claude-agent-acp 0.76.0 always lists Auto (session-mode.js buildAvailableModes).
const MODES: [&str; 4] = ["default", "acceptEdits", "plan", "auto"];

impl AcpConfig {
    pub(super) fn defaults() -> Self {
        Self {
            model: MODELS[0],
            effort: EFFORTS[1],
            mode: MODES[0],
        }
    }

    fn options(&self) -> Value {
        json!([
            {"id":"mode","name":"Mode","category":"mode","type":"select","currentValue":self.mode,
             "options":[{"value":"default","name":"Default"},{"value":"acceptEdits","name":"Accept edits"},{"value":"plan","name":"Plan"},{"value":"auto","name":"Auto"}]},
            {"id":"model","name":"Model","category":"model","type":"select","currentValue":self.model,
             "options":[{"value":MODELS[0],"name":"Synthetic replay"},{"value":MODELS[1],"name":"Synthetic replay deep"}]},
            {"id":"effort","name":"Effort","category":"thought_level","type":"select","currentValue":self.effort,
             "options":[{"value":"low","name":"Low"},{"value":"medium","name":"Medium"},{"value":"high","name":"High"}]}
        ])
    }

    pub(super) fn session(&self) -> Value {
        json!({"sessionId":SESSION,"modes":{"currentModeId":self.mode,"availableModes":[
            {"id":"default","name":"Default"},{"id":"acceptEdits","name":"Accept edits"},{"id":"plan","name":"Plan"},{"id":"auto","name":"Auto"}
        ]},"configOptions":self.options()})
    }

    pub(super) fn answer(&mut self, id: u64, request: &Value) -> Vec<Value> {
        let params = &request["params"];
        let pick = |choices: &[&'static str], value: &Value| {
            choices
                .iter()
                .copied()
                .find(|choice| Some(*choice) == value.as_str())
        };
        let refused =
            json!({"jsonrpc":"2.0","id":id,"error":{"code":-32602,"message":"Invalid value"}});
        if request["method"] == "session/set_mode" {
            let Some(mode) = pick(&MODES, &params["modeId"]) else {
                return vec![refused];
            };
            self.mode = mode;
            return vec![
                json!({"jsonrpc":"2.0","method":"session/update","params":{"sessionId":SESSION,"update":{"sessionUpdate":"config_option_update","configOptions":self.options()}}}),
                json!({"jsonrpc":"2.0","id":id,"result":{}}),
            ];
        }
        match params["configId"].as_str() {
            Some("mode") => match pick(&MODES, &params["value"]) {
                Some(mode) => self.mode = mode,
                None => return vec![refused],
            },
            Some("model") => match pick(&MODELS, &params["value"]) {
                Some(model) => self.model = model,
                None => return vec![refused],
            },
            Some("effort") => match pick(&EFFORTS, &params["value"]) {
                Some(effort) => self.effort = effort,
                None => return vec![refused],
            },
            _ => return vec![refused],
        }
        vec![json!({"jsonrpc":"2.0","id":id,"result":{"configOptions":self.options()}})]
    }
}

/// The plain session's permission modes, first being the adapter's default.
/// claude-agent-acp 0.76.0 always lists Auto.
pub(crate) const PLAIN_MODES: [&str; 3] = ["default", "plan", "auto"];

pub(crate) fn config_options(mode: &str) -> Value {
    json!([
        { "id":"mode", "name":"Mode", "category":"mode", "type":"select", "currentValue":mode,
          "options":[{"value":"default","name":"Default"},{"value":"plan","name":"Plan"},{"value":"auto","name":"Auto"}] },
        { "id":"model", "name":"Model", "category":"model", "type":"select", "currentValue":"fixture-model", "options":[{"value":"fixture-model","name":"Synthetic replay"}] }
    ])
}
