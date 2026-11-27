use serde_json::{Value, json};

/// Rows can share a name; `/name` runs the first built-in row that carries it, else the first
/// row, so only that row is listed.
pub(in crate::acp::bindings::claude::sdk) fn commands(reply: &Value) -> Value {
    let rows = reply
        .get("commands")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    fn builtin(row: &Value) -> bool {
        row.get("builtin").and_then(Value::as_bool) == Some(true)
    }
    fn name(row: &Value) -> Option<&str> {
        row.get("name").and_then(Value::as_str)
    }
    // Claude Code keeps a retired command in its catalog with a "(removed)"
    // description; a name whose running row is retired is not offered.
    let mut running = std::collections::HashMap::<&str, &Value>::new();
    for row in rows {
        if let Some(this) = name(row) {
            running
                .entry(this)
                .and_modify(|first| {
                    if !builtin(first) && builtin(row) {
                        *first = row;
                    }
                })
                .or_insert(row);
        }
    }
    let retired = |this: &str| {
        running
            .get(this)
            .and_then(|row| row.get("description").and_then(Value::as_str))
            .is_some_and(|description| description.starts_with("(removed)"))
    };
    rows.iter()
        .filter_map(|command| {
            let this = name(command).filter(|this| !retired(this))?;
            let hint = command.get("argumentHint").and_then(Value::as_str).filter(|hint| !hint.is_empty());
            let mut entry = json!({
                "name": this,
                "description": command.get("description").and_then(Value::as_str).unwrap_or_default(),
                "input": hint.map(|hint| json!({"hint": hint})),
            });
            if builtin(command) {
                entry["_meta"]["source"] = json!("builtin");
            } else if let Some((plugin, _)) = this.split_once(':') {
                entry["_meta"]["source"] = json!("plugin");
                entry["_meta"]["sourceLabel"] = json!(plugin);
            } else {
                entry["_meta"]["source"] = json!(command.get("source").and_then(Value::as_str).unwrap_or("skill"));
                if let Some(label) = command.get("sourceLabel").and_then(Value::as_str) {
                    entry["_meta"]["sourceLabel"] = json!(label);
                }
            }
            if let Some(aliases) = command.get("aliases").filter(|aliases| aliases.is_array()) {
                entry["_meta"]["aliases"] = aliases.clone();
            }
            Some(entry)
        })
        .collect()
}

/// Read only envelope fields here; both validators still check every catalog row.
#[derive(serde::Deserialize)]
struct CatalogEnvelope {
    r#type: String,
    subtype: Option<String>,
    response: Option<CatalogResponse>,
}

#[derive(serde::Deserialize)]
struct CatalogResponse {
    subtype: String,
    request_id: String,
}

impl super::SdkBridge {
    /// Sage's catalog update for commands as Sage lists them; goal support
    /// follows the same list.
    pub(super) fn catalog_update(&mut self, commands: Value) -> Value {
        let commands = self.goal_support(commands);
        self.update(
            json!({"sessionUpdate":"available_commands_update","availableCommands":commands}),
        )
    }
    pub(in crate::acp::bindings::claude::sdk) fn refreshing_commands(&self) -> bool {
        self.pending
            .values()
            .any(|pending| matches!(pending, super::Pending::Commands))
    }
    /// Claude Code 2.1.322's SDK reinitialize() sends initialize again.
    /// supportedCommands() itself only reads the cached initialize response.
    pub(in crate::acp::bindings::claude::sdk) fn refresh_commands(
        &mut self,
    ) -> Result<Value, super::BridgeError> {
        self.control(
            json!({"subtype":"initialize","perTaskStopAffordance":true,"forwardSubagentText":true}),
            super::Pending::Commands,
        )
    }
    pub(in crate::acp::bindings::claude::sdk) fn command_catalog_path(
        &self,
        bytes: &[u8],
    ) -> crate::acp::transport::CommandWirePath {
        use crate::acp::transport::CommandWirePath::{Commands, Field, Other, Unread};
        let Ok(envelope) = serde_json::from_slice::<CatalogEnvelope>(bytes) else {
            return Other;
        };
        match envelope.r#type.as_str() {
            "system" if envelope.subtype.as_deref() == Some("commands_changed") => {
                Field("commands", &Commands)
            }
            "control_response"
                if envelope.response.as_ref().is_some_and(|response| {
                    response.subtype == "success"
                        && matches!(
                            self.pending.get(&response.request_id),
                            Some(super::Pending::Initialize(_) | super::Pending::Commands)
                        )
                }) =>
            {
                Field(
                    "response",
                    &Field("response", &Field("commands", &Commands)),
                )
            }
            // A reply Sage only waits for: its payload, such as the
            // `get_settings` echo of a large managed policy, is never read.
            "control_response"
                if envelope.response.as_ref().is_some_and(|response| {
                    self.pending
                        .get(&response.request_id)
                        .is_some_and(super::Pending::discards_reply)
                }) =>
            {
                Field("response", &Field("response", &Unread))
            }
            _ => Other,
        }
    }
}
