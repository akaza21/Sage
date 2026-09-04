use super::*;

fn permission_event(ordinal: usize) -> AdapterEvent {
    let id = format!("request-{ordinal}");
    AdapterEvent::PermissionRequest {
        turn_id: identity("turn-envelope"),
        item_id: identity(&format!("item-{ordinal}")),
        external_request_id: ExternalId::new("permissionRequest", id).unwrap(),
        payload: permission(),
        request_payload_digest: [ordinal as u8; 32],
    }
}

async fn ingest_permissions(
    count: usize,
) -> (AdapterPump<FakeAdapter, FakeJournal, Mapper>, Shared) {
    let batches = (0..count)
        .map(|ordinal| owned(vec![permission_event(ordinal)], Vec::new()))
        .collect();
    let (mut pump, shared) = build_pump(batches);
    for _ in 0..count {
        pump.push_stdout_chunk(b"{}\n", 10).await.unwrap();
    }
    (pump, shared)
}

#[derive(Default)]
struct Capture(Vec<Vec<u8>>);

impl EgressTransport for Capture {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.0.push(frame.payload().to_vec());
        FrameWriteOutcome::Complete
    }
}

#[test]
fn exact_limit_cancels_each_original_id_after_every_durable_commit() {
    runtime().block_on(async {
        let limit = LIMITS.queues.server_requests_per_session as usize;
        let (mut pump, shared) = ingest_permissions(limit).await;
        shared.0.lock().unwrap().log.clear();

        assert_eq!(
            pump.queue_cancelled_permissions(
                &session("one"),
                RuntimeEpoch::new(1).unwrap(),
                &turn("one"),
                &message(),
                20,
            )
            .await
            .unwrap(),
            limit
        );
        let state = shared.0.lock().unwrap();
        assert_eq!(
            state.request_states,
            vec![ServerRequestState::Cancelled; limit]
        );
        let first_egress = state
            .log
            .iter()
            .position(|entry| *entry == "mandatory")
            .unwrap();
        assert_eq!(
            state.log[..first_egress]
                .iter()
                .filter(|entry| **entry == "publish")
                .count(),
            limit
        );
        drop(state);

        let mut capture = Capture::default();
        for _ in 0..limit {
            assert_eq!(
                pump.flush_one(&mut capture).unwrap(),
                FlushDisposition::Written
            );
        }
        assert_eq!(capture.0.len(), limit);
        for ordinal in 0..limit {
            let value: serde_json::Value = serde_json::from_slice(&capture.0[ordinal]).unwrap();
            assert_eq!(value["id"], format!("request-{ordinal}"));
            assert_eq!(value["result"]["outcome"]["outcome"], "cancelled");
        }
    });
}

#[test]
fn exact_plus_one_is_rejected_without_an_unbounded_pending_collection() {
    runtime().block_on(async {
        let limit = LIMITS.queues.server_requests_per_session as usize;
        let batches = (0..=limit)
            .map(|ordinal| owned(vec![permission_event(ordinal)], Vec::new()))
            .collect();
        let (mut pump, _) = build_pump(batches);
        for _ in 0..limit {
            pump.push_stdout_chunk(b"{}\n", 10).await.unwrap();
        }
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 10).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
    });
}

#[test]
fn foreign_owner_and_commit_failure_publish_no_cancellation_response() {
    runtime().block_on(async {
        let (mut pump, shared) = ingest_permissions(1).await;
        assert!(matches!(
            pump.queue_cancelled_permissions(
                &session("foreign"),
                RuntimeEpoch::new(1).unwrap(),
                &turn("one"),
                &message(),
                20,
            )
            .await,
            Err(AdapterPumpError::NoActiveTurn)
        ));
        assert!(shared.0.lock().unwrap().request_states.is_empty());

        shared.0.lock().unwrap().permission_failure = Some((
            ServerRequestState::Cancelled,
            PumpJournalError::CommitUnknown,
        ));
        assert!(matches!(
            pump.queue_cancelled_permissions(
                &session("one"),
                RuntimeEpoch::new(1).unwrap(),
                &turn("one"),
                &message(),
                20,
            )
            .await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(shared.0.lock().unwrap().resolutions.is_empty());
    });
}

#[test]
fn full_control_queue_fails_closed_without_writing_a_cancellation_response() {
    runtime().block_on(async {
        let queue = queue_with(1, 1_024, 1, 1_024);
        let (mut pump, shared) =
            pump_with_queue(vec![owned(vec![permission_event(1)], Vec::new())], queue);
        pump.push_stdout_chunk(b"{}\n", 10).await.unwrap();
        pump.queue_cancel_notification(
            &session("one"),
            RuntimeEpoch::new(1).unwrap(),
            &turn("one"),
            &message(),
        )
        .unwrap();

        assert!(matches!(
            pump.queue_cancelled_permissions(
                &session("one"),
                RuntimeEpoch::new(1).unwrap(),
                &turn("one"),
                &message(),
                20,
            )
            .await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        let state = shared.0.lock().unwrap();
        assert_eq!(state.request_states, [ServerRequestState::Cancelled]);
        assert_eq!(
            state.resolutions,
            [EgressResolution::Queued, EgressResolution::QueueRejected]
        );
    });
}
