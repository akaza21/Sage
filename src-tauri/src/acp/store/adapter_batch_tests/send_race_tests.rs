//! A send races idle session events: the renderer read the chat at one
//! projection revision, and session facts committed before its send arrived.

use super::*;
use crate::acp::intent::{
    ModelId,
    session_event::{
        SessionActivityState, SessionNoticeLevel, SessionScopedEvent,
        fixtures::{activity, notice, title},
    },
};

fn session(event: SessionScopedEvent) -> AdapterInboundEvent {
    AdapterInboundEvent::Session(SessionEventCommand {
        event,
        now_millis: 8,
    })
}

/// A hot session whose only Turn has completed, so a send is admissible.
fn idle_fixture() -> Fixture {
    let mut fixture = fixture();
    let turn = |state, now_millis| AdapterInboundEvent::Turn { state, now_millis };
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![turn(TurnState::Running, 6), turn(TurnState::Completed, 7)],
        ),
    )
    .unwrap();
    fixture
}

fn commit(fixture: &mut Fixture, event: SessionScopedEvent) {
    let result = apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(&fixture.owner, vec![session(event)]),
    )
    .unwrap();
    assert_eq!(result.disposition, MutationDisposition::Applied);
}

fn send(fixture: &mut Fixture, expected: ProjectionRevision) -> Result<PreparedTurn, StoreError> {
    prepare_turn(
        &mut fixture.connection,
        PrepareTurnCommand {
            project_id: fixture.owner.project_id.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: fixture.owner.chat_id.clone(),
            session_id: fixture.owner.session_id.clone(),
            runtime_epoch: fixture.owner.runtime_epoch,
            expected_projection_revision: expected,
            client_intent_id: ClientIntentId::parse("racing-send").unwrap(),
            prompt: Prompt::new("prompt", "next").unwrap(),
            now_millis: 9,
        },
    )
}

fn is_stale(result: Result<PreparedTurn, StoreError>) -> bool {
    matches!(
        result,
        Err(StoreError::Conflict {
            resource: "projectionRevision"
        })
    )
}

#[test]
fn a_send_survives_notices_titles_and_activity_committed_after_its_read() {
    let mut fixture = idle_fixture();
    let seen = projection(&fixture.connection);
    commit(
        &mut fixture,
        notice(SessionNoticeLevel::Warning, "config deprecated"),
    );
    commit(&mut fixture, title("Fix the sidebar"));
    commit(
        &mut fixture,
        SessionScopedEvent::Activity(activity("task", SessionActivityState::Running, "1/3")),
    );
    assert!(projection(&fixture.connection) > seen);

    let prepared = send(&mut fixture, seen).unwrap();
    assert_eq!(prepared.disposition, MutationDisposition::Applied);
    let duplicate = send(&mut fixture, seen).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.turn_id, prepared.turn_id);
}

#[test]
fn a_send_relevant_session_change_after_the_read_still_rejects_the_send() {
    let mut fixture = idle_fixture();
    let seen = projection(&fixture.connection);
    commit(
        &mut fixture,
        notice(SessionNoticeLevel::Info, "before the model"),
    );
    commit(
        &mut fixture,
        SessionScopedEvent::TurnModel {
            model_id: ModelId::new("modelId", "rerouted").unwrap(),
        },
    );
    commit(&mut fixture, title("After the model"));
    assert!(is_stale(send(&mut fixture, seen)));

    let after_model = projection(&fixture.connection).get() - 1;
    let prepared = send(&mut fixture, ProjectionRevision::new(after_model).unwrap()).unwrap();
    assert_eq!(prepared.disposition, MutationDisposition::Applied);
}

#[test]
fn a_send_from_a_view_ahead_of_the_chat_is_rejected() {
    let mut fixture = idle_fixture();
    commit(&mut fixture, notice(SessionNoticeLevel::Info, "neutral"));
    let ahead = projection(&fixture.connection).next().unwrap();
    assert!(is_stale(send(&mut fixture, ahead)));
}

#[test]
fn a_turn_change_after_the_read_still_rejects_the_send() {
    let mut fixture = fixture();
    let seen = projection(&fixture.connection);
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                AdapterInboundEvent::Turn {
                    state: TurnState::Running,
                    now_millis: 6,
                },
                AdapterInboundEvent::Turn {
                    state: TurnState::Completed,
                    now_millis: 7,
                },
            ],
        ),
    )
    .unwrap();
    commit(&mut fixture, title("Neutral after the terminal"));
    assert!(is_stale(send(&mut fixture, seen)));
}
