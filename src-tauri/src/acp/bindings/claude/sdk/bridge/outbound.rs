//! ACP requests from the session, written as SDK frames.

use serde_json::{Value, json};

use super::{BridgeError, Pending, SdkBridge};
use crate::acp::bindings::claude::sdk::config::{EFFORT_ID, MODE_ID, MODEL_ID, SessionSettings};

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
            // Sage shows a stop control on each background task, so a
            // cancelled turn leaves the others running.
            "initialize" => self.control(
                json!({"subtype": "initialize", "perTaskStopAffordance": true}),
                Pending::Initialize(id),
            ),
            "session/new" => self.session(id),
            "session/resume" if params["sessionId"] == self.session_id.as_str() => self.session(id),
            "session/prompt" if self.prompt.is_none() => {
                let content = prompt_content(params.get("prompt"))?;
                self.prompt = Some(id);
                self.interrupted = false;
                self.text_break = super::TextBreak::None;
                let mut user = json!({
                    "type": "user",
                    "session_id": self.session_id,
                    "message": {"role": "user", "content": content},
                    "parent_tool_use_id": null,
                });
                // The turn's id names its checkpoint, for undo.
                if let Some(uuid) = params
                    .get("messageId")
                    .and_then(Value::as_str)
                    .and_then(super::controls::message_uuid)
                {
                    user["uuid"] = uuid.into();
                }
                Ok(user)
            }
            "session/cancel" => {
                self.interrupted = self.prompt.is_some();
                self.withdraw_steers()
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
/// an attached text file becomes a text block that names its source, and a
/// linked file becomes an `@path` mention. Sage
/// attaches other files as opaque blobs, so their type is read from their
/// leading bytes: images and PDFs go as such, anything else is named but
/// not sent, since Claude Code accepts no other binary content.
fn prompt_content(blocks: Option<&Value>) -> Result<Vec<Value>, BridgeError> {
    blocks
        .and_then(Value::as_array)
        .ok_or(BridgeError::Unsupported)?
        .iter()
        .map(|block| match block.get("type").and_then(Value::as_str) {
            Some("text") => Ok(json!({"type": "text", "text": text(block.get("text"))?})),
            Some("image") => {
                let data = text(block.get("data"))?;
                match sniff(data) {
                    Some(media_type) if media_type.starts_with("image/") => {
                        Ok(base64_block("image", media_type, data))
                    }
                    _ => Err(BridgeError::Unsupported),
                }
            }
            Some("resource") => {
                let resource = block.get("resource").ok_or(BridgeError::Unsupported)?;
                let uri = text(resource.get("uri"))?;
                let Some(blob) = resource.get("blob") else {
                    let body = text(resource.get("text"))?;
                    let context = format!("<context ref=\"{uri}\">\n{body}\n</context>");
                    return Ok(json!({"type": "text", "text": context}));
                };
                let blob = text(Some(blob))?;
                Ok(match sniff(blob) {
                    Some("application/pdf") => base64_block("document", "application/pdf", blob),
                    Some(media_type) => base64_block("image", media_type, blob),
                    None => json!({"type": "text", "text": format!(
                        "<context ref=\"{uri}\">\n(Binary file not shown: only images and PDFs can be sent to Claude Code.)\n</context>"
                    )}),
                })
            }
            // A file named but not attached: Claude Code reads an `@path`
            // mention itself.
            Some("resource_link") => {
                let uri = text(block.get("uri"))?;
                let path = url::Url::parse(uri)
                    .ok()
                    .filter(|url| url.scheme() == "file")
                    .and_then(|url| url.to_file_path().ok())
                    .ok_or(BridgeError::Unsupported)?;
                // Claude Code reads an `@path` itself, so a credential file
                // Sage would not embed is named without one.
                if crate::acp::service::prompt_files::secrets::path_is_secret(&path, false) {
                    let context = format!(
                        "<context ref=\"{uri}\">\n(Not attached: Sage does not send credential files.)\n</context>"
                    );
                    return Ok(json!({"type": "text", "text": context}));
                }
                let path = path.display().to_string();
                let mention = if path.contains(char::is_whitespace) {
                    format!("@\"{path}\"")
                } else {
                    format!("@{path}")
                };
                Ok(json!({"type": "text", "text": mention}))
            }
            _ => Err(BridgeError::Unsupported),
        })
        .collect()
}

/// The media type of a base64 file Claude accepts, from its leading bytes.
fn sniff(blob: &str) -> Option<&'static str> {
    use base64::Engine;
    let head = base64::engine::general_purpose::STANDARD
        .decode(blob.get(..16)?)
        .ok()?;
    match head.as_slice() {
        [0x89, b'P', b'N', b'G', ..] => Some("image/png"),
        [0xFF, 0xD8, 0xFF, ..] => Some("image/jpeg"),
        [b'G', b'I', b'F', b'8', ..] => Some("image/gif"),
        [b'R', b'I', b'F', b'F', _, _, _, _, b'W', b'E', b'B', b'P'] => Some("image/webp"),
        [b'%', b'P', b'D', b'F', ..] => Some("application/pdf"),
        _ => None,
    }
}

fn base64_block(kind: &str, media_type: &str, data: &str) -> Value {
    json!({"type": kind, "source": {"type": "base64", "media_type": media_type, "data": data}})
}
