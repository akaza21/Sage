use super::*;

fn permission_event(id: &str) -> AdapterEvent {
    AdapterEvent::PermissionRequest {
        turn_id: identity("turn-envelope"),
        item_id: identity(&format!("item-{id}")),
        external_request_id: ExternalId::new("permissionRequest", id).expect("request id"),
        payload: permission(),
        request_payload_digest: [7; 32],
    }
}

#[test]
fn same_batch_permission_and_terminal_enters_teardown_without_response_or_handoff() {
    runtime().block_on(async {
        let batch = owned(
            vec![
                permission_event("same-batch"),
                terminal(TurnState::Completed),
            ],
            Vec::new(),
        );
        let (mut pump, shared) = build_pump(vec![batch]);

        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 10).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(pump.teardown_required());
        assert!(!pump.completed_turn_ready());
        assert!(pump.try_into_completed_parts().is_err());

        let state = shared.0.lock().expect("shared state");
        assert!(state.durable_batches.is_empty());
        assert!(state.commands.is_empty());
        assert!(state.resolutions.is_empty());
    });
}

#[test]
fn permission_before_terminal_blocks_terminal_commit_and_handoff_without_response() {
    runtime().block_on(async {
        let permission = owned(vec![permission_event("before-terminal")], Vec::new());
        let terminal = owned(vec![terminal(TurnState::Completed)], Vec::new());
        let (mut pump, shared) = build_pump(vec![permission, terminal]);

        pump.push_stdout_chunk(b"{}\n", 10)
            .await
            .expect("permission ingress");
        assert!(!pump.completed_turn_ready());
        assert!(matches!(
            pump.push_stdout_chunk(b"{}\n", 11).await,
            Err(AdapterPumpError::TeardownRequired)
        ));
        assert!(pump.teardown_required());
        assert!(!pump.completed_turn_ready());
        assert!(pump.try_into_completed_parts().is_err());

        let state = shared.0.lock().expect("shared state");
        assert_eq!(state.durable_batches.len(), 1);
        assert!(matches!(
            state.durable_batches[0].as_slice(),
            [DurableAdapterEvent::PermissionRequest { .. }]
        ));
        assert!(state.commands.is_empty());
        assert!(state.resolutions.is_empty());
    });
}
