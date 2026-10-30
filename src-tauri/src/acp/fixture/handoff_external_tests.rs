// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use super::{external_fixture_adapters::ExternalCase, handoff_support::HandoffJourney};
use crate::acp::{context_handoff::HandoffState, runtime::LiveWrite, store::SendObservation};

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn handoff_inline_reaches_fresh_native_session_once() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut journey = HandoffJourney::new().await;
        journey.complete().await;
        let old = journey.session_id();
        journey = journey
            .switch("binding-b", ExternalCase::HandoffInline)
            .await;
        assert_ne!(journey.session_id(), old);
        journey.complete().await;
        assert_eq!(journey.summary().state, HandoffState::Inline);
        assert_eq!(journey.summary().carried_items, 2);
        assert_eq!(journey.send_count(), 1);
        assert_eq!(journey.original_user_text(), "read only");
        journey.finish().await;
    });
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn handoff_zero_byte_retry_preserves_plan() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut journey = HandoffJourney::new().await;
        journey.complete().await;
        journey = journey
            .switch("binding-b", ExternalCase::ZeroByteFailure)
            .await;
        let plan = journey.plan();
        assert_eq!(
            journey.dispatch(SendObservation::ZeroBytes).await,
            LiveWrite::ZeroBytes
        );
        assert_eq!(journey.summary().state, HandoffState::Prepared);
        assert_eq!(
            journey.dispatch(SendObservation::ZeroBytes).await,
            LiveWrite::ZeroBytes
        );
        assert_eq!(journey.plan(), plan);
        assert_eq!(journey.send_count(), 2);
        journey.finish().await;
    });
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn handoff_crash_after_send_recovers_unknown_without_resend() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut journey = HandoffJourney::new().await;
        journey.complete().await;
        journey = journey
            .switch(
                "binding-b",
                ExternalCase::CompleteDeliveryExit("send-complete-uncorrelated-exit"),
            )
            .await;
        let plan = journey.plan();
        assert_eq!(
            journey.dispatch(SendObservation::CompleteBytes).await,
            LiveWrite::Complete
        );
        assert_eq!(journey.summary().state, HandoffState::Pending);
        journey.crash_reopen().await;
        assert_eq!(journey.summary().state, HandoffState::Unknown);
        assert_eq!(journey.send_count(), 1);
        assert_eq!(journey.plan(), plan);
        journey.assert_resend_refused();
        journey.finish().await;
    });
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn handoff_a_b_a_uses_three_native_sessions() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut journey = HandoffJourney::new().await;
        let first = journey.session_id();
        journey.complete().await;
        journey = journey
            .switch("binding-b", ExternalCase::HandoffInline)
            .await;
        let second = journey.session_id();
        journey.complete().await;
        journey = journey
            .switch("fixture-binding", ExternalCase::HandoffInline)
            .await;
        let third = journey.session_id();
        journey.complete().await;
        assert_ne!(first, second);
        assert_ne!(second, third);
        assert_ne!(first, third);
        assert_eq!(journey.native_epoch_count(), 3);
        assert_eq!(journey.summary().carried_items, 4);
        journey.finish().await;
    });
}

#[test]
#[ignore = "requires the separately built, path-bound acp-adversarial-fixture"]
fn handoff_other_live_lane_remains_usable() {
    tokio::runtime::Runtime::new().unwrap().block_on(async {
        let mut first = HandoffJourney::new().await;
        let mut other = first.other_lane().await;
        let owner = other.session_id();
        first.complete().await;
        first = first.switch("binding-b", ExternalCase::HandoffInline).await;
        first.complete().await;
        other.complete().await;
        assert_eq!(other.session_id(), owner);
        assert_eq!(other.send_count(), 1);
        assert_eq!(first.summary().state, HandoffState::Inline);
        other.finish().await;
        first.finish().await;
    });
}
