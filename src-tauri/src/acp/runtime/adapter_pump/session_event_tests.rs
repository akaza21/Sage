use super::*;
use crate::acp::intent::session_event::{
    PendingSessionEvents, SessionNoticeLevel, SessionScopedEvent,
    fixtures::{notice as notice_fixture, title},
};

/// The exact session identity: no Turn correlation at all.
fn session_identity() -> AdapterIdentity {
    AdapterIdentity {
        binding_digest: [7; 32],
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        session_id: session("one"),
        external_id: ExternalId::new("session", "external-session").unwrap(),
        turn_id: None,
        turn_client_message_id: None,
    }
}

fn notice(text: &str) -> SessionScopedEvent {
    notice_fixture(SessionNoticeLevel::Warning, text)
}

fn session_event(id: AdapterIdentity, event: SessionScopedEvent) -> AdapterEvent {
    AdapterEvent::Session { id, event }
}

fn durable_notices(shared: &Shared) -> Vec<String> {
    shared
        .0
        .lock()
        .unwrap()
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::Session {
                scope,
                event: SessionScopedEvent::Notice(notice),
                ..
            } if scope.session_id == session("one") && scope.binding_digest == "07".repeat(32) => {
                Some(notice.text.as_str().to_owned())
            }
            _ => None,
        })
        .collect()
}

#[test]
fn session_events_are_admitted_without_a_turn_identity_during_and_after_the_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(
                vec![session_event(session_identity(), notice("during"))],
                Vec::new(),
            ),
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(
                vec![
                    session_event(session_identity(), notice("after terminal")),
                    item("late-item"),
                ],
                Vec::new(),
            ),
        ]);
        assert!(pump.push_stdout_chunk(b"{}\n{}\n{}\n", 1).await.unwrap());
        assert!(pump.drain_stdout(2).await.unwrap());
        assert!(pump.drain_stdout(3).await.unwrap());
        assert_eq!(durable_notices(&shared), ["during", "after terminal"]);
        // The late turn-scoped item is still rejected after the terminal.
        assert!(
            !shared
                .0
                .lock()
                .unwrap()
                .durable_batches
                .iter()
                .flatten()
                .any(|event| matches!(event, DurableAdapterEvent::Item { .. }))
        );
    });
}

#[test]
fn session_events_with_a_turn_or_foreign_identity_are_dropped() {
    runtime().block_on(async {
        let mut foreign = session_identity();
        foreign.external_id = ExternalId::new("session", "other-session").unwrap();
        let (mut pump, shared) = build_pump(vec![owned(
            vec![
                session_event(identity("turn-scoped"), notice("turn identity")),
                session_event(foreign, notice("foreign session")),
            ],
            Vec::new(),
        )]);
        assert!(pump.push_stdout_chunk(b"{}\n", 1).await.unwrap());
        assert!(durable_notices(&shared).is_empty());
    });
}

#[test]
fn a_completed_turn_commits_idle_session_events_and_drops_foreign_ones() {
    runtime().block_on(async {
        let mut foreign = session_identity();
        foreign.runtime_epoch = RuntimeEpoch::new(2).unwrap();
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(
                vec![
                    session_event(session_identity(), notice("between turns")),
                    session_event(foreign, notice("stale epoch")),
                ],
                Vec::new(),
            ),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let mut idle = pump.try_into_completed_parts().ok().unwrap();
        idle.accept_idle_stdout_chunk(b"{}\n", 2, &mut CaptureTransport(Vec::new()))
            .await
            .unwrap();
        assert_eq!(durable_notices(&shared), ["between turns"]);
        assert_eq!(shared.0.lock().unwrap().log.last(), Some(&"publish"));
    });
}

#[test]
fn idle_session_events_in_one_chunk_commit_once_with_superseded_titles_dropped() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            owned(
                vec![
                    session_event(session_identity(), title("Draft")),
                    session_event(session_identity(), notice("first")),
                ],
                Vec::new(),
            ),
            owned(
                vec![
                    session_event(session_identity(), title("Final")),
                    session_event(session_identity(), notice("second")),
                ],
                Vec::new(),
            ),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let mut idle = pump.try_into_completed_parts().ok().unwrap();
        let batches_before = shared.0.lock().unwrap().durable_batches.len();
        idle.accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut CaptureTransport(Vec::new()))
            .await
            .unwrap();

        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), batches_before + 1);
        let titles = state.durable_batches[batches_before]
            .iter()
            .filter_map(|event| match event {
                DurableAdapterEvent::Session {
                    event: SessionScopedEvent::Title { title },
                    ..
                } => Some(title.as_str().to_owned()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, ["Final"]);
        drop(state);
        assert_eq!(durable_notices(&shared), ["first", "second"]);
    });
}

#[test]
fn events_buffered_before_the_first_turn_commit_with_its_first_batch() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![owned(vec![item("first")], Vec::new())]);
        let mut pending = PendingSessionEvents::default();
        pending.push(title("Old title"));
        pending.push(notice("from activation"));
        pending.push(title("New title"));
        pump.seed_session_events(pending, 1);
        assert!(shared.0.lock().unwrap().durable_batches.is_empty());
        pump.push_stdout_chunk_deferred(b"{}\n", 100).await.unwrap();
        pump.flush_deferred().await.unwrap();
        let state = shared.0.lock().unwrap();
        assert_eq!(state.durable_batches.len(), 1);
        let titles = state.durable_batches[0]
            .iter()
            .filter_map(|event| match event {
                DurableAdapterEvent::Session {
                    event: SessionScopedEvent::Title { title },
                    ..
                } => Some(title.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(titles, ["New title"]);
        drop(state);
        assert_eq!(durable_notices(&shared), ["from activation"]);
    });
}
