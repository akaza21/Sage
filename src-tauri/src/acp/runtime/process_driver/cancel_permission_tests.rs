use super::*;

fn permission(id: &str) -> AdapterEvent {
    AdapterEvent::PermissionRequest {
        turn_id: identity_one("turn-envelope"),
        item_id: identity_one(&format!("item-{id}")),
        external_request_id: ExternalId::new("permissionRequest", id).expect("request"),
        payload: NormalizedPayload::PermissionRequest {
            category: crate::acp::intent::ToolCategory::Other,
            summary: SafeText::new("summary", "permission").expect("summary"),
            offered_option_kinds: None,
        },
        request_payload_digest: [5; 32],
    }
}

async fn ingest(driver: &mut TestDriver, count: usize) {
    let mailbox = DriverMailbox::new().expect("mailbox");
    for _ in 0..count {
        mailbox.try_push_chunk(b"{}\n".to_vec()).expect("chunk");
        driver.run_next(&mailbox).await.expect("permission ingress");
    }
}

#[test]
fn zero_pending_permissions_writes_only_the_ordinary_cancel_notification() {
    runtime().block_on(async {
        let (mut driver, shared, _, _) = build_driver(Vec::new(), VecDeque::new());

        let progress = driver
            .dispatch_cancel(cancel("one", "message-one"))
            .await
            .expect("cancel");

        assert_eq!(progress.cancelled_permission_frames, 0);
        assert_eq!(progress.written_frames, 1);
        assert_eq!(shared.0.lock().expect("shared").commands, ["cancel"]);
    });
}

#[test]
fn driver_writes_all_cancelled_permissions_before_one_cancel_notification() {
    runtime().block_on(async {
        let batches = vec![
            owned(vec![permission("request-one")]),
            owned(vec![permission("request-two")]),
        ];
        let (mut driver, shared, _, _) = build_driver(batches, VecDeque::new());
        ingest(&mut driver, 2).await;

        let progress = driver
            .dispatch_cancel(cancel("one", "message-one"))
            .await
            .expect("cancel");

        assert_eq!(progress.cancelled_permission_frames, 2);
        assert_eq!(progress.written_frames, 3);
        assert_eq!(driver.transport.writes, 3);
        let state = shared.0.lock().expect("shared");
        assert_eq!(
            state.permission_states,
            [ServerRequestState::Cancelled, ServerRequestState::Cancelled]
        );
        assert_eq!(
            state.commands,
            ["cancelPermission", "cancelPermission", "cancel"]
        );
    });
}

#[test]
fn failed_permission_response_tears_down_before_cancel_notification() {
    runtime().block_on(async {
        let (mut driver, shared, _, control) = build_driver(
            vec![owned(vec![permission("request-one")])],
            VecDeque::from([FrameWriteOutcome::ZeroByteFailure]),
        );
        ingest(&mut driver, 1).await;

        assert!(matches!(
            driver.dispatch_cancel(cancel("one", "message-one")).await,
            Err(DriverError::TeardownRequired(_))
        ));
        assert_eq!(
            shared.0.lock().expect("shared").commands,
            ["cancelPermission"]
        );
        assert_eq!(control.lock().expect("control").requested.len(), 1);
    });
}

#[test]
fn unknown_permission_response_delivery_tears_down_without_replay_or_cancel_notification() {
    runtime().block_on(async {
        let (mut driver, shared, _, control) = build_driver(
            vec![owned(vec![permission("request-one")])],
            VecDeque::from([FrameWriteOutcome::DeliveryUnknown { bytes_written: 1 }]),
        );
        ingest(&mut driver, 1).await;

        assert!(matches!(
            driver.dispatch_cancel(cancel("one", "message-one")).await,
            Err(DriverError::TeardownRequired(
                TeardownCause::DeliveryUnknown
            ))
        ));
        assert_eq!(
            shared.0.lock().expect("shared").commands,
            ["cancelPermission"]
        );
        assert_eq!(control.lock().expect("control").requested.len(), 1);
        assert!(driver.first_delivery_unknown().is_some());
    });
}
