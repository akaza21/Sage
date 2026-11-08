use std::{collections::BTreeMap, io::ErrorKind, path::Path};

use serde_json::{Value, json};

use crate::acp::file_diff::DIFF_SIDE_BYTES_MAX;

use super::super::{PermissionIntent, PermissionReply, PermissionState};

pub(super) fn target_absent(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_err_and(|error| error.kind() == ErrorKind::NotFound)
}

pub(super) fn created_file_content(
    request: &Value,
    approved_path: Option<&Path>,
) -> Option<String> {
    let path = approved_path?;
    if !target_absent(path) {
        return None;
    }
    let content = request.pointer("/params/toolCall/content")?.as_array()?;
    let [entry] = content.as_slice() else {
        return None;
    };
    if entry.get("type")?.as_str()? != "diff"
        || entry.get("path")?.as_str()? != path.to_str()?
        || !entry.get("oldText")?.as_str()?.is_empty()
    {
        return None;
    }
    let new_text = entry.get("newText")?.as_str()?;
    (new_text.len() <= DIFF_SIDE_BYTES_MAX).then(|| new_text.to_owned())
}

pub(super) fn attach_completed_created_file(
    update: &mut Value,
    permissions: &BTreeMap<String, PermissionIntent>,
    turn_request_id: Option<u64>,
) {
    let Some(turn_request_id) = turn_request_id else {
        return;
    };
    let tool_id = match update.pointer("/params/update") {
        Some(payload)
            if payload.get("sessionUpdate").and_then(Value::as_str) == Some("tool_call_update")
                && payload.get("status").and_then(Value::as_str) == Some("completed") =>
        {
            payload.get("toolCallId").and_then(Value::as_str)
        }
        _ => None,
    };
    let Some(tool_id) = tool_id else {
        return;
    };
    let Some(permission) = permissions.values().find(|record| {
        record.tool_id == tool_id
            && record.turn_request_id == turn_request_id
            && matches!(
                record.reply,
                PermissionReply::AllowOnce | PermissionReply::AllowAlways
            )
            && record.state == PermissionState::Written
            && record.client_write_used
            && record.created_file_written
    }) else {
        return;
    };
    let (Some(path), Some(content)) = (
        permission
            .approved_path
            .as_ref()
            .and_then(|path| path.to_str()),
        permission.created_file_content.as_deref(),
    ) else {
        return;
    };
    let Some(payload) = update
        .pointer_mut("/params/update")
        .and_then(Value::as_object_mut)
    else {
        return;
    };
    let entries = payload
        .entry("content")
        .or_insert_with(|| Value::Array(Vec::new()));
    let Some(entries) = entries.as_array_mut() else {
        return;
    };
    if entries
        .iter()
        .any(|entry| entry.get("type").and_then(Value::as_str) == Some("diff"))
    {
        return;
    }
    entries.push(json!({"type":"diff","path":path,"newText":content}));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn permission() -> PermissionIntent {
        PermissionIntent {
            translator_id: "number:0".to_owned(),
            tool_id: "tool".to_owned(),
            request_digest: [0; 32],
            wire_id: json!(0),
            allow_once_option_id: Some("once".to_owned()),
            allow_always_option_id: None,
            reply: PermissionReply::AllowOnce,
            state: PermissionState::Written,
            turn_request_id: 3,
            approved_path: Some("C:\\workspace\\created.txt".into()),
            client_write_used: true,
            created_file_content: Some("created".to_owned()),
            created_file_written: true,
        }
    }

    fn completed(tool_id: &str) -> Value {
        json!({
            "params": {"update": {
                "sessionUpdate": "tool_call_update",
                "status": "completed",
                "toolCallId": tool_id,
                "content": []
            }}
        })
    }

    #[test]
    fn wrong_tool_or_turn_cannot_inherit_a_created_file_diff() {
        let permissions = BTreeMap::from([("request".to_owned(), permission())]);
        for (tool_id, turn_id) in [("other", Some(3)), ("tool", Some(4)), ("tool", None)] {
            let mut update = completed(tool_id);
            attach_completed_created_file(&mut update, &permissions, turn_id);
            assert_eq!(update["params"]["update"]["content"], json!([]));
        }
    }

    #[test]
    fn differing_or_failed_write_cannot_inherit_a_created_file_diff() {
        let mut differing = permission();
        differing.created_file_written = false;
        let mut failed = permission();
        failed.client_write_used = false;
        failed.created_file_written = false;
        for record in [differing, failed] {
            let permissions = BTreeMap::from([("request".to_owned(), record)]);
            let mut update = completed("tool");
            attach_completed_created_file(&mut update, &permissions, Some(3));
            assert_eq!(update["params"]["update"]["content"], json!([]));
        }
    }
}
