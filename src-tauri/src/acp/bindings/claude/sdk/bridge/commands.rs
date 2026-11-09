//! Claude Code's slash commands as ACP `availableCommands`.

use std::collections::HashSet;

use serde_json::{Value, json};

use crate::acp::slash_commands::MAX_COMMANDS;

/// Claude Code's commands as ACP `availableCommands`, its own marked as
/// built-in. Rows can share a name; `/name` runs the first built-in row that
/// carries it, else the first row, so only that row is listed. A catalog
/// larger than Sage's list keeps its first commands.
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
    let marked: HashSet<&str> = rows
        .iter()
        .filter(|row| builtin(row))
        .filter_map(name)
        .collect();
    let mut listed = HashSet::new();
    rows.iter()
        .filter_map(|command| {
            let this = name(command)?;
            let runs = builtin(command) || !marked.contains(this);
            if !runs || !listed.insert(this) {
                return None;
            }
            let hint = command.get("argumentHint").and_then(Value::as_str).filter(|hint| !hint.is_empty());
            let mut entry = json!({
                "name": this,
                "description": command.get("description").and_then(Value::as_str).unwrap_or_default(),
                "input": hint.map(|hint| json!({"hint": hint})),
            });
            if builtin(command) {
                entry["_meta"] = json!({"source": "builtin"});
            }
            Some(entry)
        })
        .take(MAX_COMMANDS)
        .collect()
}
