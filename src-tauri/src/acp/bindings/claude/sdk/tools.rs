//! Claude Code tool uses as ACP tool-call fields.
//!
//! Claude names its built-in tools and their input fields; this table is the
//! only place that knows them. Unknown and MCP tools keep their raw input and
//! a neutral kind so the shared translator still renders them.

use serde_json::{Map, Value, json};

/// The ACP fields of one tool use: `title`, `kind`, `content`, `locations`,
/// `rawInput` and the `_meta.claudeCode.toolName` the translator reads for
/// subagent rows.
pub(super) fn tool_fields(name: &str, input: &Value) -> Map<String, Value> {
    let text = |key: &str| input.get(key).and_then(Value::as_str);
    let path = text("file_path").or_else(|| text("notebook_path"));
    let (kind, title, content) = match name {
        "Read" => ("read", titled("Read", path), Vec::new()),
        "Write" => (
            "edit",
            titled("Write", path),
            diff(path, None, text("content")),
        ),
        "Edit" => (
            "edit",
            titled("Edit", path),
            diff(path, text("old_string"), text("new_string")),
        ),
        "MultiEdit" => (
            "edit",
            titled("Edit", path),
            input
                .get("edits")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .flat_map(|edit| {
                    diff(
                        path,
                        edit.get("old_string").and_then(Value::as_str),
                        edit.get("new_string").and_then(Value::as_str),
                    )
                })
                .collect(),
        ),
        "NotebookEdit" => (
            "edit",
            titled("Edit", path),
            diff(path, None, text("new_source")),
        ),
        "Bash" | "PowerShell" => (
            "execute",
            text("command").unwrap_or(name).to_owned(),
            Vec::new(),
        ),
        "Grep" => ("search", titled("Search", text("pattern")), Vec::new()),
        "Glob" => ("search", titled("Find", text("pattern")), Vec::new()),
        "WebFetch" => ("fetch", titled("Fetch", text("url")), Vec::new()),
        "WebSearch" => (
            "fetch",
            titled("Search the web for", text("query")),
            Vec::new(),
        ),
        "Task" | "Agent" => (
            "think",
            text("description").unwrap_or("Subagent").to_owned(),
            Vec::new(),
        ),
        "ExitPlanMode" => (
            "switch_mode",
            "Ready to code?".to_owned(),
            text("plan").map(text_content).into_iter().collect(),
        ),
        _ if name.starts_with("mcp__") => ("mcp", mcp_title(name), Vec::new()),
        _ => ("other", name.to_owned(), Vec::new()),
    };
    let mut fields = Map::new();
    fields.insert("title".into(), title.into());
    fields.insert("kind".into(), kind.into());
    fields.insert("content".into(), content.into());
    fields.insert(
        "locations".into(),
        path.map(|path| json!([{ "path": path }]))
            .unwrap_or_else(|| json!([])),
    );
    fields.insert("rawInput".into(), input.clone());
    fields.insert(
        "_meta".into(),
        json!({ "claudeCode": { "toolName": name } }),
    );
    fields
}

/// `TodoWrite` input as ACP plan entries, or `None` for any other tool.
pub(super) fn plan_entries(name: &str, input: &Value) -> Option<Value> {
    if name != "TodoWrite" {
        return None;
    }
    let entries = input
        .get("todos")?
        .as_array()?
        .iter()
        .filter_map(|todo| {
            let status = todo.get("status")?.as_str()?;
            let content = match todo.get("activeForm").and_then(Value::as_str) {
                Some(active) if status == "in_progress" => active,
                _ => todo.get("content")?.as_str()?,
            };
            Some(json!({ "content": content, "status": status, "priority": "medium" }))
        })
        .collect::<Vec<_>>();
    Some(entries.into())
}

/// A `tool_result` block's text, joined across its text parts.
pub(super) fn result_text(content: Option<&Value>) -> Option<String> {
    match content? {
        Value::String(text) => Some(text.clone()),
        Value::Array(blocks) => {
            let text = blocks
                .iter()
                .filter(|block| block.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|block| block.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n");
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

pub(super) fn text_content(text: &str) -> Value {
    json!({ "type": "content", "content": { "type": "text", "text": text } })
}

fn titled(verb: &str, subject: Option<&str>) -> String {
    subject.map_or_else(|| verb.to_owned(), |subject| format!("{verb} {subject}"))
}

fn diff(path: Option<&str>, old_text: Option<&str>, new_text: Option<&str>) -> Vec<Value> {
    match (path, new_text) {
        (Some(path), Some(new_text)) => vec![json!({
            "type": "diff",
            "path": path,
            "oldText": old_text,
            "newText": new_text,
        })],
        _ => Vec::new(),
    }
}

/// `mcp__server__tool` reads as `server: tool`.
fn mcp_title(name: &str) -> String {
    name.strip_prefix("mcp__")
        .and_then(|rest| rest.split_once("__"))
        .map_or_else(
            || name.to_owned(),
            |(server, tool)| format!("{server}: {tool}"),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edit_becomes_an_edit_with_a_fragment_diff_and_location() {
        let fields = tool_fields(
            "Edit",
            &json!({"file_path":"/w/a.rs","old_string":"a","new_string":"b"}),
        );
        assert_eq!(fields["kind"], "edit");
        assert_eq!(fields["title"], "Edit /w/a.rs");
        assert_eq!(
            fields["content"],
            json!([{"type":"diff","path":"/w/a.rs","oldText":"a","newText":"b"}])
        );
        assert_eq!(fields["locations"], json!([{"path":"/w/a.rs"}]));
    }

    #[test]
    fn write_is_a_created_file_diff() {
        let fields = tool_fields("Write", &json!({"file_path":"/w/n.txt","content":"x\n"}));
        assert_eq!(fields["content"][0]["oldText"], Value::Null);
        assert_eq!(fields["content"][0]["newText"], "x\n");
    }

    #[test]
    fn bash_title_is_the_command_and_subagents_keep_their_tool_name() {
        assert_eq!(
            tool_fields("Bash", &json!({"command":"ls -la"}))["title"],
            "ls -la"
        );
        let task = tool_fields("Task", &json!({"description":"Map the code"}));
        assert_eq!(task["_meta"]["claudeCode"]["toolName"], "Task");
        assert_eq!(task["title"], "Map the code");
    }

    #[test]
    fn mcp_tools_name_their_server() {
        let fields = tool_fields("mcp__github__get_issue", &json!({}));
        assert_eq!(
            (fields["kind"].as_str(), fields["title"].as_str()),
            (Some("mcp"), Some("github: get_issue"))
        );
    }

    #[test]
    fn todo_write_shows_the_active_form_of_the_running_item() {
        let entries = plan_entries(
            "TodoWrite",
            &json!({"todos":[
                {"content":"Write tests","activeForm":"Writing tests","status":"in_progress"},
                {"content":"Ship","activeForm":"Shipping","status":"pending"}
            ]}),
        )
        .unwrap();
        assert_eq!(entries[0]["content"], "Writing tests");
        assert_eq!(entries[1]["content"], "Ship");
        assert!(plan_entries("Read", &json!({})).is_none());
    }

    #[test]
    fn result_text_joins_text_blocks_only() {
        let content =
            json!([{"type":"text","text":"a"},{"type":"image"},{"type":"text","text":"b"}]);
        assert_eq!(result_text(Some(&content)).as_deref(), Some("a\nb"));
        assert_eq!(result_text(Some(&json!("plain"))).as_deref(), Some("plain"));
        assert_eq!(result_text(Some(&json!([]))), None);
    }
}
