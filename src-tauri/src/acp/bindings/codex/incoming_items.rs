use serde_json::{Map, Value};

use crate::acp::{
    contract::{BoundedVec, SafeText},
    image_admit::{ImageAdmitError, admit_image_data_url},
    intent::{NormalizedPayload, ToolCategory},
    lifecycle::{ItemLifecycle, ToolState},
    limits::LIMITS,
    runtime::host_browse::{CURATED_BROWSE_TOOL_NAME, admit_browse_url},
};

use super::super::CODEX_BROWSE_TOOL_NAME;
use super::{
    IngressError, ResourceMapper,
    support::{content, safe_text, text, unsupported},
};

type ItemNormalization = (NormalizedPayload, Option<ToolState>, Option<ItemLifecycle>);

pub(super) fn normalize_item<M: ResourceMapper>(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    mapper: &M,
) -> Result<Option<ItemNormalization>, IngressError> {
    let kind = text(item, "type", 128)?;
    match kind {
        // Sage persists the client prompt before dispatching `turn/start`.
        // Codex subsequently echoes that same user-facing item, so retaining
        // it again would duplicate the user bubble. Keep the echo out of the
        // durable projection without treating a known protocol item as an
        // unsupported activity.
        "userMessage" => Ok(None),
        "agentMessage" => Ok(Some((
            NormalizedPayload::Text {
                text: safe_text(item.get("text"), 262_144, "text")?,
            },
            None,
            None,
        ))),
        "plan" => Ok(Some((
            NormalizedPayload::Plan {
                summary: safe_text(item.get("text"), 262_144, "plan")?,
            },
            None,
            None,
        ))),
        "reasoning" => Ok(Some((
            NormalizedPayload::ThoughtSummary {
                summary: SafeText::new("summary", "Reasoning summary available")
                    .map_err(content)?,
            },
            None,
            None,
        ))),
        "commandExecution" => tool(item, lifecycle, ToolCategory::Command, "Command").map(Some),
        "mcpToolCall" => tool(item, lifecycle, ToolCategory::Mcp, "MCP tool").map(Some),
        "dynamicToolCall" => dynamic_tool(item, lifecycle).map(Some),
        "webSearch" => tool(item, lifecycle, ToolCategory::Web, "Web search").map(Some),
        "collabAgentToolCall" | "subAgentActivity" => Ok(Some((
            NormalizedPayload::Collaboration {
                label: SafeText::new("activityLabel", "Harness-owned child activity")
                    .map_err(content)?,
                message: None,
                receiver_labels: BoundedVec::new("receivers", vec![]).map_err(content)?,
            },
            None,
            None,
        ))),
        "fileChange" => file_change(item, mapper).map(Some),
        "imageGeneration" => image_generation(item, lifecycle),
        other => Ok(Some((
            unsupported(other, "Unsupported Codex item")?,
            None,
            Some(ItemLifecycle::Unsupported),
        ))),
    }
}

fn dynamic_tool(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<ItemNormalization, IngressError> {
    let label = text(item, "tool", 128)?;
    if label == CURATED_BROWSE_TOOL_NAME || label == CODEX_BROWSE_TOOL_NAME {
        let href = item
            .get("arguments")
            .and_then(Value::as_object)
            .and_then(|arguments| arguments.get("url"))
            .and_then(Value::as_str)
            .and_then(admit_browse_url)
            .ok_or(IngressError::Malformed)?;
        return tool_with_summary(
            item,
            lifecycle,
            ToolCategory::Web,
            CURATED_BROWSE_TOOL_NAME,
            Some(href),
        );
    }
    tool(item, lifecycle, ToolCategory::Other, "Tool")
}

fn image_generation(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
) -> Result<Option<ItemNormalization>, IngressError> {
    match item.get("result") {
        None | Some(Value::Null) if !lifecycle.is_terminal() => Ok(None),
        None | Some(Value::Null) => {
            rejected_image("imageGeneration", ImageAdmitError::MissingBytes)
        }
        Some(Value::String(url)) => match admit_image_data_url(url) {
            Ok(payload) => Ok(Some((payload, None, Some(ItemLifecycle::Completed)))),
            Err(reason) => rejected_image("imageGeneration", reason),
        },
        Some(_) => rejected_image("imageGeneration", ImageAdmitError::InvalidBase64),
    }
}

fn rejected_image(
    kind: &str,
    reason: ImageAdmitError,
) -> Result<Option<ItemNormalization>, IngressError> {
    Ok(Some((
        unsupported(kind, reason.unsupported_summary())?,
        None,
        Some(ItemLifecycle::Unsupported),
    )))
}

fn tool(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    category: ToolCategory,
    label: &str,
) -> Result<ItemNormalization, IngressError> {
    tool_with_summary(item, lifecycle, category, label, None)
}

fn tool_with_summary(
    item: &Map<String, Value>,
    lifecycle: ItemLifecycle,
    category: ToolCategory,
    label: &str,
    summary: Option<String>,
) -> Result<ItemNormalization, IngressError> {
    let status = item
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("inProgress");
    let tool_state = match status {
        "inProgress" => ToolState::InProgress,
        "completed" => ToolState::Completed,
        "failed" => ToolState::Failed,
        "declined" => ToolState::Declined,
        _ => return Err(IngressError::Malformed),
    };
    let terminal = match tool_state {
        ToolState::Completed => Some(ItemLifecycle::Completed),
        ToolState::Failed => Some(ItemLifecycle::Failed),
        ToolState::Declined => Some(ItemLifecycle::Cancelled),
        _ => None,
    };
    let label = SafeText::new("toolLabel", label).map_err(content)?;
    let payload = if lifecycle == ItemLifecycle::Started {
        NormalizedPayload::ToolCall {
            category,
            label,
            input_summary: summary
                .map(|value| SafeText::new("toolInput", value))
                .transpose()
                .map_err(content)?,
        }
    } else {
        NormalizedPayload::ToolUpdateResult {
            category,
            label,
            output_summary: summary
                .map(|value| SafeText::new("toolOutput", value))
                .transpose()
                .map_err(content)?,
        }
    };
    Ok((payload, Some(tool_state), terminal))
}

fn file_change<M: ResourceMapper>(
    item: &Map<String, Value>,
    mapper: &M,
) -> Result<ItemNormalization, IngressError> {
    let changes = item
        .get("changes")
        .and_then(Value::as_array)
        .ok_or(IngressError::Malformed)?;
    if changes.len() != 1 {
        return Ok((
            unsupported("fileChange", "File change spans multiple resources")?,
            None,
            Some(ItemLifecycle::Unsupported),
        ));
    }
    let change = changes[0].as_object().ok_or(IngressError::Malformed)?;
    let absolute = text(
        change,
        "path",
        LIMITS.wire.absolute_path_bytes_native_only as usize,
    )?;
    let resource = match mapper.map_existing(absolute) {
        Ok(resource) => crate::acp::intent::SafeResourceLocation::Resource { resource },
        Err(reason) => crate::acp::intent::SafeResourceLocation::unavailable(reason),
    };
    Ok((
        NormalizedPayload::FileChangeDiff {
            resource,
            summary: SafeText::new("fileSummary", "Codex reported a file change")
                .map_err(content)?,
            additions: None,
            deletions: None,
        },
        Some(ToolState::Completed),
        Some(ItemLifecycle::Completed),
    ))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use crate::acp::intent::{
        IMAGE_BASE64_MAX, ImageMediaType, SafeResourceRef, UnavailableResourceReason,
    };

    use super::*;

    const PNG_1X1: &str = "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

    struct UnusedMapper;

    impl ResourceMapper for UnusedMapper {
        fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
            Err(UnavailableResourceReason::Unsupported)
        }
    }

    fn item(value: Value) -> Map<String, Value> {
        value.as_object().cloned().expect("object item")
    }

    #[test]
    fn image_generation_admits_png_data_url() {
        let item = item(json!({
            "type": "imageGeneration",
            "id": "img-1",
            "status": "completed",
            "result": format!("data:image/png;base64,{PNG_1X1}")
        }));
        let (payload, tool_state, lifecycle) =
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .expect("admitted image");
        assert!(tool_state.is_none());
        assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
        let NormalizedPayload::Image { media_type, data } = payload else {
            panic!("expected image payload");
        };
        assert_eq!(media_type, ImageMediaType::Png);
        assert_eq!(data.as_str(), PNG_1X1);
    }

    #[test]
    fn image_generation_rejects_svg_as_unsupported() {
        let item = item(json!({
            "type": "imageGeneration",
            "id": "img-svg",
            "result": "data:image/svg+xml;base64,PHN2Zy8+"
        }));
        let (payload, _, lifecycle) =
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .expect("rejected image");
        assert_eq!(lifecycle, Some(ItemLifecycle::Unsupported));
        let NormalizedPayload::Unsupported {
            external_kind,
            summary,
        } = payload
        else {
            panic!("expected unsupported");
        };
        assert_eq!(external_kind.as_str(), "imageGeneration");
        assert_eq!(summary.as_str(), "Image media type is not admitted");
    }

    #[test]
    fn image_generation_rejects_oversize_without_partial_bytes() {
        let item = item(json!({
            "type": "imageGeneration",
            "id": "img-big",
            "result": format!("data:image/png;base64,{}", "A".repeat(IMAGE_BASE64_MAX))
        }));
        let (payload, _, lifecycle) =
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .expect("rejected image");
        assert_eq!(lifecycle, Some(ItemLifecycle::Unsupported));
        assert!(matches!(
            payload,
            NormalizedPayload::Unsupported { ref summary, .. }
                if summary.as_str() == "Image exceeds the bounded size cap"
        ));
    }

    #[test]
    fn image_generation_rejects_missing_bytes_on_completed() {
        let item = item(json!({
            "type": "imageGeneration",
            "id": "img-empty",
            "status": "completed"
        }));
        let (payload, _, lifecycle) =
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .expect("rejected image");
        assert_eq!(lifecycle, Some(ItemLifecycle::Unsupported));
        assert!(matches!(
            payload,
            NormalizedPayload::Unsupported { ref summary, .. }
                if summary.as_str() == "Image payload is missing"
        ));
    }

    #[test]
    fn image_generation_skips_in_progress_without_bytes() {
        let item = item(json!({
            "type": "imageGeneration",
            "id": "img-pending",
            "status": "inProgress"
        }));
        assert!(
            normalize_item(&item, ItemLifecycle::Started, &UnusedMapper)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn unknown_codex_item_stays_unsupported() {
        let item = item(json!({
            "type": "imageView",
            "id": "view-1",
            "path": r"C:\tmp\x.png"
        }));
        let (payload, _, lifecycle) =
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .expect("unsupported view");
        assert_eq!(lifecycle, Some(ItemLifecycle::Unsupported));
        assert!(matches!(
            payload,
            NormalizedPayload::Unsupported { ref external_kind, .. }
                if external_kind.as_str() == "imageView"
        ));
    }

    #[test]
    fn user_message_echo_is_ignored_after_sage_persists_the_prompt() {
        let item = item(json!({
            "type": "userMessage",
            "id": "user-echo-1",
            "text": "Already represented by the local prompt"
        }));
        assert!(
            normalize_item(&item, ItemLifecycle::Started, &UnusedMapper)
                .unwrap()
                .is_none()
        );
        assert!(
            normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn dynamic_browse_keeps_its_curated_label_and_admitted_url() {
        let item = item(json!({
            "type": "dynamicToolCall",
            "id": "browse-1",
            "tool": "sage_browse",
            "arguments": {"url": "http://127.0.0.1:43124/proof"},
            "status": "inProgress"
        }));
        let (payload, state, terminal) =
            normalize_item(&item, ItemLifecycle::Started, &UnusedMapper)
                .unwrap()
                .expect("admitted browse");
        assert_eq!(state, Some(ToolState::InProgress));
        assert_eq!(terminal, None);
        assert!(matches!(
            payload,
            NormalizedPayload::ToolCall { category: ToolCategory::Web, label, input_summary: Some(summary) }
                if label.as_str() == "sage.browse" && summary.as_str() == "http://127.0.0.1:43124/proof"
        ));
    }
}
