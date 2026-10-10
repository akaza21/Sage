use super::*;

fn idle_response(token: EgressToken) -> AdapterBatch {
    owned(
        vec![AdapterEvent::IgnoredLate],
        vec![AdapterEgress::new(
            OutboundFrame::new(br#"{"cancelled":true}"#.to_vec()).unwrap(),
            token,
        )],
    )
}

#[test]
fn completed_idle_pump_writes_only_mandatory_cancellations_and_preserves_delivery_stages() {
    runtime().block_on(async {
        let cancellation =
            EgressToken::permission_cancel(ExternalId::new("request", "dialog").unwrap());
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            idle_response(cancellation),
        ]);
        pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
        let mut idle = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        idle.accept_idle_stdout_chunk(b"{}\n", 2, &mut transport)
            .await
            .unwrap();
        assert_eq!(transport.0, br#"{"cancelled":true}"#);
        let state = shared.0.lock().unwrap();
        assert!(state.log.contains(&"mandatory"));
        assert_eq!(
            state.resolutions,
            [EgressResolution::Queued, EgressResolution::Written]
        );
    });
}

#[test]
fn completed_idle_pump_still_rejects_requests_notifications_allows_and_aborts_without_writing() {
    runtime().block_on(async {
        for token in [
            EgressToken::request(1),
            EgressToken::protocol_notification(),
            EgressToken::permission_allow(ExternalId::new("request", "gate").unwrap()),
            EgressToken::mandatory_deny(ExternalId::new("request", "gate").unwrap(), false),
        ] {
            let (mut pump, shared) = build_pump(vec![
                owned(vec![terminal(TurnState::Completed)], Vec::new()),
                idle_response(token),
            ]);
            pump.push_stdout_chunk(b"{}\n", 1).await.unwrap();
            let mut idle = pump.try_into_completed_parts().ok().unwrap();
            let mut transport = CaptureTransport(Vec::new());
            assert!(matches!(
                idle.accept_idle_stdout_chunk(b"{}\n", 2, &mut transport)
                    .await,
                Err(AdapterPumpError::Frame)
            ));
            assert!(transport.0.is_empty());
            assert!(shared.0.lock().unwrap().resolutions.is_empty());
        }
    });
}
