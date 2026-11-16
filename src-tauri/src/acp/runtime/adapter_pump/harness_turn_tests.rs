use super::*;

/// The exact session identity a harness names a Turn it started with.
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

fn started() -> AdapterBatch {
    owned(
        vec![AdapterEvent::TurnStarted {
            id: session_identity(),
        }],
        Vec::new(),
    )
}

fn next_item(external: &str) -> AdapterBatch {
    let next = next_turn();
    owned(
        vec![AdapterEvent::Item {
            id: AdapterIdentity {
                turn_id: next.external_turn_id,
                turn_client_message_id: Some(next.client_message_id),
                ..identity(external)
            },
            lifecycle: ItemLifecycle::Streaming,
            tool_state: None,
            payload: text(external),
        }],
        Vec::new(),
    )
}

fn durable_items(shared: &Shared) -> Vec<String> {
    shared
        .0
        .lock()
        .unwrap()
        .durable_batches
        .iter()
        .flatten()
        .filter_map(|event| match event {
            DurableAdapterEvent::Item {
                external_item_id, ..
            } => Some(external_item_id.as_str().to_owned()),
            _ => None,
        })
        .collect()
}

#[test]
fn a_turn_the_harness_starts_after_the_terminal_keeps_its_stdout_for_the_next_turn() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            next_item("first"),
            next_item("second"),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n{}\n", 2)
            .await
            .unwrap();
        assert!(pump.next_turn_started());
        assert!(!pump.drain_stdout_deferred(3).await.unwrap());
        assert!(pump.has_pending_stdout_input(), "the second frame waits");
        assert!(pump.completed_turn_ready());

        let parts = pump.try_into_completed_parts().ok().unwrap();
        assert!(parts.turn_started());
        let (parts, _) = *parts
            .bind_turn(next_turn())
            .err()
            .expect("a prompt cannot take the started Turn's place");
        let mut next = parts.bind_harness_turn(next_turn()).ok().unwrap();
        next.adopt_started_turn(4).await.unwrap();
        assert!(next.drain_stdout_deferred(5).await.unwrap());
        next.flush_deferred().await.unwrap();

        assert_eq!(durable_items(&shared), ["first", "second"]);
        assert!(!next.teardown_required());
    });
}

#[test]
fn a_turn_the_harness_starts_while_idle_is_bound_with_the_stdout_after_it() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            next_item("first"),
            next_item("second"),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        parts
            .accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut transport)
            .await
            .unwrap();
        assert!(parts.turn_started());
        assert!(
            parts
                .accept_idle_stdout_chunk(b"{}\n", 3, &mut transport)
                .await
                .is_err(),
            "nothing more is read before the Turn opens"
        );

        let mut next = parts.bind_harness_turn(next_turn()).ok().unwrap();
        next.adopt_started_turn(4).await.unwrap();
        assert!(next.drain_stdout_deferred(5).await.unwrap());
        assert!(
            !next.drain_stdout_deferred(6).await.unwrap(),
            "the refused chunk was never buffered"
        );
        next.flush_deferred().await.unwrap();

        assert_eq!(durable_items(&shared), ["first", "second"]);
        assert!(transport.0.is_empty());
    });
}

#[test]
fn a_run_the_harness_starts_before_this_turns_prompt_reaches_it_is_declined() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![started(), owned(vec![item("own")], Vec::new())]);
        pump.push_stdout_chunk_deferred(b"{}\n{}\n", 1)
            .await
            .unwrap();
        assert!(pump.drain_stdout_deferred(2).await.unwrap());
        pump.flush_deferred().await.unwrap();
        assert!(!pump.next_turn_started());
        assert!(!pump.teardown_required());
        assert_eq!(
            shared.0.lock().unwrap().log,
            ["decline", "journal", "publish"]
        );
        assert_eq!(
            pump.egress.data_usage().0,
            1,
            "the interrupt waits to be written"
        );
        assert_eq!(durable_items(&shared), ["own"]);
    });
}

#[test]
fn a_declined_turn_reads_on_through_the_stdout_after_it() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
            owned(vec![AdapterEvent::Absorbed], Vec::new()),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        let mut parts = pump.try_into_completed_parts().ok().unwrap();
        let mut transport = CaptureTransport(Vec::new());
        parts
            .accept_idle_stdout_chunk(b"{}\n{}\n", 2, &mut transport)
            .await
            .unwrap();
        assert!(parts.turn_started());

        parts.decline_started_turn(3, &mut transport).await.unwrap();
        assert!(!parts.turn_started());
        assert_eq!(transport.0, br#"{"interrupt":true}"#);
        assert!(
            shared.0.lock().unwrap().batches.is_empty(),
            "the frame after it was read"
        );
        assert!(parts.bind_turn(next_turn()).is_ok(), "a prompt may follow");
    });
}

#[test]
fn a_started_turn_is_not_bound_while_a_setting_changes() {
    runtime().block_on(async {
        let (mut pump, shared) = build_pump(vec![
            owned(vec![terminal(TurnState::Completed)], Vec::new()),
            started(),
        ]);
        pump.push_stdout_chunk_deferred(b"{}\n", 1).await.unwrap();
        pump.push_stdout_chunk_deferred(b"{}\n", 2).await.unwrap();
        assert!(pump.next_turn_started());
        let parts = pump.try_into_completed_parts().ok().unwrap();
        shared.0.lock().unwrap().config_pending = true;
        let (parts, _) = *parts.bind_harness_turn(next_turn()).err().unwrap();
        shared.0.lock().unwrap().config_pending = false;
        assert!(parts.bind_harness_turn(next_turn()).is_ok());
    });
}
