use serde_json::json;

use crate::acp::intent::{
    IMAGE_BASE64_MAX, ImageMediaType, SafeResourceRef, UnavailableResourceReason,
};

use super::*;

const PNG_1X1: &str =
    "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+ip1sAAAAASUVORK5CYII=";

struct UnusedMapper;

impl ResourceMapper for UnusedMapper {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        Err(UnavailableResourceReason::Unsupported)
    }
}

struct MappedResource;

impl ResourceMapper for MappedResource {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        SafeResourceRef::new(
            crate::acp::contract::SageRootId::parse("fixture-root").expect("root"),
            "package.json",
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

struct CreatedResource;

impl ResourceMapper for CreatedResource {
    fn map_existing(&self, _: &str) -> Result<SafeResourceRef, UnavailableResourceReason> {
        SafeResourceRef::new(
            crate::acp::contract::SageRootId::parse("fixture-root").expect("root"),
            "src/new.ts",
        )
        .map_err(|_| UnavailableResourceReason::Unsupported)
    }
}

#[test]
fn codex_added_file_projects_created_with_a_safe_relative_name() {
    let fixture: Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/features/chat/acp/createdFileEvent.fixture.json"
    )))
    .expect("fixture");
    let file = item(fixture["raw"].clone());
    let (payload, _, _) = normalize_item(&file, ItemLifecycle::Completed, &CreatedResource)
        .expect("file change")
        .expect("item");
    let NormalizedPayload::FileChangeDiff {
        operation, summary, ..
    } = payload
    else {
        panic!("expected file change");
    };
    assert_eq!(operation, crate::acp::intent::FileChangeOperation::Created);
    assert_eq!(
        summary.as_str(),
        fixture["expected"]["summary"].as_str().unwrap()
    );
    assert_eq!(
        serde_json::to_value(operation).expect("operation"),
        fixture["expected"]["operation"]
    );
}

#[test]
fn codex_unmapped_changes_keep_the_operation_without_exposing_a_path() {
    for (kind, expected_operation, expected_summary) in [
        (
            "add",
            crate::acp::intent::FileChangeOperation::Created,
            "Created file",
        ),
        (
            "delete",
            crate::acp::intent::FileChangeOperation::Deleted,
            "Deleted file",
        ),
    ] {
        let file = item(json!({
            "type": "fileChange",
            "id": "file-unmapped",
            "status": "completed",
            "changes": [{
                "path": "C:\\outside\\secret.ts",
                "kind": {"type": kind}
            }]
        }));
        let (payload, _, _) = normalize_item(&file, ItemLifecycle::Completed, &UnusedMapper)
            .expect("file change")
            .expect("item");
        let NormalizedPayload::FileChangeDiff {
            operation, summary, ..
        } = payload
        else {
            panic!("expected file change");
        };
        assert_eq!(operation, expected_operation);
        assert_eq!(summary.as_str(), expected_summary);
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
    assert_eq!(
        serde_json::to_value(&payload).unwrap()["payload"]["source"],
        "agent"
    );
    assert!(tool_state.is_none());
    assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
    let NormalizedPayload::Image {
        media_type, data, ..
    } = payload
    else {
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
    let (payload, _, lifecycle) = normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
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
    let (payload, _, lifecycle) = normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
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
    let (payload, _, lifecycle) = normalize_item(&item, ItemLifecycle::Completed, &UnusedMapper)
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
fn an_item_type_this_build_never_heard_of_still_reaches_other_with_its_raw_title() {
    let unheard_of = item(json!({
        "type": "codexItemTypeFromALaterRelease",
        "id": "future-1",
        "title": "Ran a future thing",
        "somethingNobodyModelled": {"depth": 2}
    }));
    let (payload, state, lifecycle) =
        normalize_item(&unheard_of, ItemLifecycle::Completed, &UnusedMapper)
            .expect("never rejected")
            .expect("never silently dropped");
    assert_eq!(state, Some(ToolState::Completed));
    assert_eq!(lifecycle, Some(ItemLifecycle::Completed));
    let NormalizedPayload::ToolCall {
        category,
        label,
        input_summary,
        ..
    } = payload
    else {
        panic!("expected a visible other tool");
    };
    assert_eq!(category, ToolCategory::Other);
    assert_eq!(label.as_str(), "Ran a future thing");
    let input = input_summary.expect("raw input preserved");
    assert!(input.as_str().contains("codexItemTypeFromALaterRelease"));
    assert!(input.as_str().contains("somethingNobodyModelled"));
}

#[test]
fn an_untitled_unknown_item_falls_back_to_its_raw_type_and_stays_pending_until_terminal() {
    let untitled = item(json!({"type": "hookPrompt", "id": "hook-1"}));
    let (payload, state, lifecycle) =
        normalize_item(&untitled, ItemLifecycle::Started, &UnusedMapper)
            .expect("never rejected")
            .expect("never silently dropped");
    assert_eq!(state, Some(ToolState::InProgress));
    assert_eq!(lifecycle, None);
    assert!(matches!(
        payload,
        NormalizedPayload::ToolCall { category: ToolCategory::Other, ref label, .. }
            if label.as_str() == "hookPrompt"
    ));
}

#[test]
fn image_view_is_its_own_kind_that_keeps_its_clickable_path() {
    let view = item(json!({
        "type": "imageView",
        "id": "view-1",
        "status": "completed",
        "path": "C:\\fixture\\package.json"
    }));
    let (payload, state, _) = normalize_item(&view, ItemLifecycle::Completed, &MappedResource)
        .expect("image view")
        .expect("item");
    assert_eq!(state, Some(ToolState::Completed));
    let NormalizedPayload::ToolCall {
        category,
        label,
        locations,
        ..
    } = payload
    else {
        panic!("expected a read tool");
    };
    assert_eq!(category, ToolCategory::ImageView);
    assert_eq!(label.as_str(), "Viewed an image");
    assert!(
        matches!(locations.as_slice(), [crate::acp::intent::SafeResourceLocation::Resource { resource }]
            if resource.relative_path.as_str() == "package.json")
    );
}

#[test]
fn review_mode_transitions_are_switch_mode_tools_carrying_the_review() {
    for (kind, expected) in [
        ("enteredReviewMode", "Entered review mode"),
        ("exitedReviewMode", "Exited review mode"),
    ] {
        let transition = item(json!({
            "type": kind,
            "id": format!("review-{kind}"),
            "review": "Check the adapter contract"
        }));
        let (payload, state, lifecycle) =
            normalize_item(&transition, ItemLifecycle::Completed, &UnusedMapper)
                .expect("review transition")
                .expect("item");
        assert_eq!(state, Some(ToolState::Completed), "{kind}");
        assert_eq!(lifecycle, Some(ItemLifecycle::Completed), "{kind}");
        assert!(
            matches!(
                payload,
                NormalizedPayload::ToolCall {
                    category: ToolCategory::SwitchMode,
                    ref label,
                    input_summary: Some(ref review),
                    ..
                } if label.as_str() == expected
                    && review.as_str() == "Check the adapter contract"
            ),
            "{kind}"
        );
    }
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

mod integration_tests;
mod tool_tests;
