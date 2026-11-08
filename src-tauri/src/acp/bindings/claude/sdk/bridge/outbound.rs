//! ACP requests from the session, written as SDK frames.

use serde_json::{Value, json};

use super::{BridgeError, Pending, SdkBridge};
use crate::acp::bindings::claude::sdk::config::{EFFORT_ID, MODE_ID, MODEL_ID, SessionSettings};

pub(in crate::acp::bindings::claude::sdk) const ALLOW_ONCE: &str = "allow_once";
pub(in crate::acp::bindings::claude::sdk) const ALLOW_ALWAYS: &str = "allow_always";
pub(in crate::acp::bindings::claude::sdk) const REJECT: &str = "reject";
const DECLINED: &str = "The user declined this tool use.";

impl SdkBridge {
    /// The SDK frame for one ACP frame the session wants to send. Every ACP
    /// frame maps to exactly one SDK frame, so delivery outcomes stay exact.
    pub(in crate::acp::bindings::claude::sdk) fn outbound(
        &mut self,
        acp: &Value,
    ) -> Result<Value, BridgeError> {
        let id = acp.get("id").cloned().unwrap_or(Value::Null);
        let params = acp.get("params").unwrap_or(&Value::Null);
        let Some(method) = acp.get("method").and_then(Value::as_str) else {
            return self.permission_reply(&id, acp.get("result"));
        };
        match method {
            "initialize" => self.control(json!({"subtype": "initialize"}), Pending::Initialize(id)),
            "session/new" => self.session(id),
            "session/resume" if params["sessionId"] == self.session_id.as_str() => self.session(id),
            "session/prompt" if self.prompt.is_none() => {
                let content = prompt_content(params.get("prompt"))?;
                self.prompt = Some(id);
                self.interrupted = false;
                Ok(json!({
                    "type": "user",
                    "session_id": self.session_id,
                    "message": {"role": "user", "content": content},
                    "parent_tool_use_id": null,
                }))
            }
            "session/cancel" => {
                self.interrupted = self.prompt.is_some();
                self.control(json!({"subtype": "interrupt"}), Pending::Interrupt)
            }
            "session/set_mode" => {
                let mode = text(params.get("modeId"))?;
                self.require(|settings| settings.has_mode(mode))?;
                let request = json!({"subtype": "set_permission_mode", "mode": mode});
                let mode = mode.to_owned();
                self.control(request, Pending::Mode { id, mode })
            }
            "session/set_config_option" => self.config_option(id, params),
            _ => Err(BridgeError::Unsupported),
        }
    }

    fn session(&mut self, id: Value) -> Result<Value, BridgeError> {
        self.control(json!({"subtype": "get_settings"}), Pending::Session(id))
    }

    fn config_option(&mut self, id: Value, params: &Value) -> Result<Value, BridgeError> {
        let config_id = text(params.get("configId"))?;
        let value = text(params.get("value"))?;
        let request = match config_id {
            MODE_ID => {
                self.require(|settings| settings.has_mode(value))?;
                json!({"subtype": "set_permission_mode", "mode": value})
            }
            MODEL_ID => {
                self.require(|settings| settings.has_model(value))?;
                json!({"subtype": "set_model", "model": value})
            }
            EFFORT_ID => {
                self.require(|settings| settings.has_effort(value))?;
                json!({
                    "subtype": "apply_flag_settings",
                    "settings": {"effortLevel": SessionSettings::effort_setting(value)},
                })
            }
            _ => return Err(BridgeError::Unsupported),
        };
        let pending = Pending::Option {
            id,
            config_id: config_id.to_owned(),
            value: value.to_owned(),
        };
        self.control(request, pending)
    }

    /// A decision on a `can_use_tool` request. Allow keeps the tool's input;
    /// allow-always also adds Claude Code's suggested permission rules.
    fn permission_reply(
        &mut self,
        id: &Value,
        result: Option<&Value>,
    ) -> Result<Value, BridgeError> {
        let wire = id.as_u64().ok_or(BridgeError::Unsupported)?;
        let permission = self
            .permissions
            .remove(&wire)
            .ok_or(BridgeError::Unsupported)?;
        let outcome = result
            .and_then(|result| result.get("outcome"))
            .ok_or(BridgeError::Unsupported)?;
        let selected = (outcome.get("outcome").and_then(Value::as_str) == Some("selected"))
            .then(|| outcome.get("optionId").and_then(Value::as_str))
            .flatten();
        let response = match selected {
            Some(ALLOW_ONCE) => json!({"behavior": "allow", "updatedInput": permission.input}),
            Some(ALLOW_ALWAYS) => json!({
                "behavior": "allow",
                "updatedInput": permission.input,
                "updatedPermissions": permission.suggestions.unwrap_or_else(|| json!([])),
            }),
            _ => json!({"behavior": "deny", "message": DECLINED}),
        };
        Ok(json!({
            "type": "control_response",
            "response": {"subtype": "success", "request_id": permission.request_id, "response": response},
        }))
    }

    fn require(&self, check: impl FnOnce(&SessionSettings) -> bool) -> Result<(), BridgeError> {
        self.settings
            .as_ref()
            .filter(|settings| check(settings))
            .map(drop)
            .ok_or(BridgeError::Unsupported)
    }
}

fn text(value: Option<&Value>) -> Result<&str, BridgeError> {
    value
        .and_then(Value::as_str)
        .ok_or(BridgeError::Unsupported)
}

/// ACP prompt blocks as Messages API content: text and images pass through;
/// an attached text file becomes a text block that names its source.
fn prompt_content(blocks: Option<&Value>) -> Result<Vec<Value>, BridgeError> {
    blocks
        .and_then(Value::as_array)
        .ok_or(BridgeError::Unsupported)?
        .iter()
        .map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => Ok(json!({"type": "text", "text": text(block.get("text"))?})),
            Some("image") => Ok(json!({
                "type": "image",
                "source": {
                    "type": "base64",
                    "media_type": text(block.get("mimeType"))?,
                    "data": text(block.get("data"))?,
                },
            })),
            Some("resource") => {
                let resource = block.get("resource").ok_or(BridgeError::Unsupported)?;
                Ok(json!({
                    "type": "text",
                    "text": format!(
                        "<context ref=\"{}\">\n{}\n</context>",
                        text(resource.get("uri"))?,
                        text(resource.get("text"))?,
                    ),
                }))
            }
            _ => Err(BridgeError::Unsupported),
        })
        .collect()
}
