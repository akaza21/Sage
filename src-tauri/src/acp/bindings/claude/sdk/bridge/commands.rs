//! Claude Code's slash commands as ACP `availableCommands`.

use serde_json::{Value, json};

/// Claude Code's commands as ACP `availableCommands`, its own marked as
/// built-in. Rows can share a name; `/name` runs the built-in row when one
/// carries it, else the first, so only that row is listed.
pub(super) fn commands(reply: &Value) -> Value {
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
    rows.iter()
        .enumerate()
        .filter_map(|(index, command)| {
            let this = name(command)?;
            let runs = |other: &Value| name(other) == Some(this);
            let shadowed = if builtin(command) {
                rows[..index].iter().any(|row| runs(row) && builtin(row))
            } else {
                rows.iter().any(|row| runs(row) && builtin(row))
                    || rows[..index].iter().any(runs)
            };
            if shadowed {
                return None;
            }
            let hint = command.get("argumentHint").and_then(Value::as_str).filter(|hint| !hint.is_empty());
            let mut listed = json!({
                "name": this,
                "description": command.get("description").and_then(Value::as_str).unwrap_or_default(),
                "input": hint.map(|hint| json!({"hint": hint})),
            });
            if builtin(command) {
                listed["_meta"] = json!({"source": "builtin"});
            }
            Some(listed)
        })
        .collect()
}
