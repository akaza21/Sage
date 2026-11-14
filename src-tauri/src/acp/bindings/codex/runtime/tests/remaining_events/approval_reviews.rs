use super::*;
use crate::acp::intent::session_event::{SESSION_NOTICES_MAX, SessionSurface};

fn completion(turn: &str, review: &str) -> Value {
    json!({"threadId":"thread-1","turnId":turn,"reviewId":review,
        "action":{"type":"command"},
        "review":{"status":"denied","rationale":format!("Review {review}.")}})
}

fn complete_review(runtime: &mut CodexRuntime, turn: &str, review: &str) -> Vec<AdapterEvent> {
    notify(
        runtime,
        "item/autoApprovalReview/completed",
        completion(turn, review),
    )
}

#[test]
fn interleaved_review_replays_stay_suppressed_after_notice_eviction() {
    let mut runtime = running();
    let mut surface = SessionSurface::default();
    let first = one_notice(&complete_review(&mut runtime, "turn-1", "A"));
    let _ = surface.apply(1, &SessionScopedEvent::Notice(first.clone()));
    let second = one_notice(&complete_review(&mut runtime, "turn-1", "B"));
    let _ = surface.apply(1, &SessionScopedEvent::Notice(second));
    assert!(matches!(
        complete_review(&mut runtime, "turn-1", "A").as_slice(),
        [AdapterEvent::IgnoredLate]
    ));

    for index in 0..SESSION_NOTICES_MAX {
        let notice = one_notice(&complete_review(
            &mut runtime,
            "turn-1",
            &format!("other-{index}"),
        ));
        let _ = surface.apply(1, &SessionScopedEvent::Notice(notice));
    }
    assert_eq!(surface.notices.len(), SESSION_NOTICES_MAX);
    assert!(!surface.notices.contains(&first));
    let retained = surface.clone();
    let replay = complete_review(&mut runtime, "turn-1", "A");
    for event in &replay {
        if let AdapterEvent::Session { event, .. } = event {
            let _ = surface.apply(1, event);
        }
    }
    assert_eq!(surface, retained);
    assert!(matches!(replay.as_slice(), [AdapterEvent::IgnoredLate]));
}

#[test]
fn review_capacity_suppresses_new_notices_without_evicting_completed_ids() {
    let mut runtime = running();
    for index in 0..sage_core::limits::LIMITS.wire.items_per_turn {
        one_notice(&complete_review(
            &mut runtime,
            "turn-1",
            &format!("review-{index}"),
        ));
    }
    for review in ["overflow", "review-0", "overflow"] {
        assert!(matches!(
            complete_review(&mut runtime, "turn-1", review).as_slice(),
            [AdapterEvent::IgnoredLate]
        ));
    }

    notify(
        &mut runtime,
        "turn/completed",
        json!({"threadId":"thread-1","turn":{"id":"turn-1","status":"completed"}}),
    );
    assert!(matches!(
        complete_review(&mut runtime, "turn-1", "review-0").as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    let prompt = runtime
        .command(AdapterCommand::Prompt {
            prompt: Prompt::new("prompt", "Continue").unwrap(),
            client_message_id: RequiredText::new("client", "client-2").unwrap(),
            access_mode: crate::acp_protocol::access_mode::SessionAccessMode::ReadOnly,
        })
        .unwrap();
    runtime
        .ingest(
            &frame(json!({"id":request_id(&prompt),"result":{"turn":{"id":"turn-2"}}})),
            &Mapper,
        )
        .unwrap();
    one_notice(&complete_review(&mut runtime, "turn-2", "review-0"));
    assert!(matches!(
        complete_review(&mut runtime, "turn-1", "review-0").as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
    assert!(matches!(
        complete_review(&mut runtime, "turn-2", "review-0").as_slice(),
        [AdapterEvent::IgnoredLate]
    ));
}
