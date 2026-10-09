use super::{catalog_page_crossing_turn, run_catalog_page_crossing_turn};
use serde_json::json;

#[test]
fn cold_first_prompt_applies_mid_turn_catalog_and_exposes_effort_after_completion() {
    catalog_page_crossing_turn(true, true, None, false);
}

#[test]
fn catalog_page_crosses_response_started_and_completed_without_active_pagination() {
    catalog_page_crossing_turn(false, true, None, false);
}

#[test]
fn catalog_page_after_turn_completion_updates_effort_without_teardown() {
    catalog_page_crossing_turn(true, false, None, false);
}

#[test]
fn deferred_catalog_cursor_resumes_at_next_idle_point() {
    catalog_page_crossing_turn(false, true, None, true);
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum CatalogFailure {
    BeforePrompt,
    DuringTurn,
    AfterTurn,
}

#[test]
fn catalog_error_before_prompt_preserves_empty_or_partial_catalog_and_live_session() {
    for cold_prompt in [true, false] {
        catalog_page_crossing_turn(
            cold_prompt,
            false,
            Some(CatalogFailure::BeforePrompt),
            false,
        );
    }
}

#[test]
fn catalog_error_during_turn_preserves_empty_or_partial_catalog_and_live_session() {
    for cold_prompt in [true, false] {
        catalog_page_crossing_turn(cold_prompt, true, Some(CatalogFailure::DuringTurn), false);
    }
}

#[test]
fn catalog_error_after_turn_preserves_empty_or_partial_catalog_and_live_session() {
    for cold_prompt in [true, false] {
        catalog_page_crossing_turn(cold_prompt, false, Some(CatalogFailure::AfterTurn), false);
    }
}

#[test]
fn invalid_catalog_page_idle_preserves_partial_catalog_and_stops_paging() {
    rejected_catalog_pages(Some(CatalogFailure::BeforePrompt), false);
}

#[test]
fn invalid_catalog_page_mid_turn_preserves_partial_catalog_and_stops_paging() {
    rejected_catalog_pages(Some(CatalogFailure::DuringTurn), true);
}

#[test]
fn invalid_catalog_page_after_turn_preserves_partial_catalog_and_stops_paging() {
    rejected_catalog_pages(Some(CatalogFailure::AfterTurn), false);
}

fn rejected_catalog_pages(failure: Option<CatalogFailure>, during_turn: bool) {
    let model = json!({"id":"replacement","displayName":"Replacement"});
    let too_many = (0..129)
        .map(|index| {
            json!({
                "id":format!("model-{index}"),"displayName":"Model"
            })
        })
        .collect::<Vec<_>>();
    let oversized_efforts = (0..32)
        .map(|index| {
            json!({
                "reasoningEffort":format!("effort-{index}"),"description":"x".repeat(8192)
            })
        })
        .collect::<Vec<_>>();
    let structural_overflow = (0..513)
        .map(|index| {
            json!({
                "id":format!("model-{index}"),"displayName":"Model"
            })
        })
        .collect::<Vec<_>>();
    let invalid_pages = [
        json!({"data":[{"id":"selected","displayName":"Selected",
            "defaultReasoningEffort":"low","supportedReasoningEfforts":[{
                "reasoningEffort":"low","description":"Quick","unexpected":true
            }]}],"nextCursor":"third"}),
        json!({"data":[model.clone(),model.clone()],"nextCursor":"third"}),
        json!({"data":[model],"nextCursor":"x".repeat(513)}),
        json!({"data":too_many,"nextCursor":"third"}),
        json!({"data":structural_overflow,"nextCursor":"third"}),
        json!({"data":too_many[..128],"nextCursor":"third"}),
        json!({"data":[{"id":"selected","displayName":"Selected",
            "supportedReasoningEfforts":oversized_efforts}],"nextCursor":"third"}),
        json!({"data":[{"id":"","displayName":"Invalid"}],"nextCursor":"third"}),
        json!("not a result object"),
    ];
    let runtime = tokio::runtime::Runtime::new().unwrap();
    for result in invalid_pages {
        runtime.block_on(run_catalog_page_crossing_turn(
            false,
            during_turn,
            failure,
            false,
            Some(result),
        ));
    }
}
