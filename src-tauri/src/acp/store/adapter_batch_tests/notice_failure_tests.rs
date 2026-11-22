use super::*;

#[test]
fn native_failure_preserves_the_notice_rendering_contract() {
    let mut fixture = migrated_fixture();
    apply_adapter_inbound_batch(
        &mut fixture.connection,
        batch(
            &fixture.owner,
            vec![
                terminal_item(
                    &fixture.owner,
                    "failure-item",
                    NormalizedPayload::Error {
                        kind: None,
                        code: SafeText::new("code", "harnessExited").unwrap(),
                        message: SafeText::new("message", "The harness exited unexpectedly.")
                            .unwrap(),
                        retryable: true,
                    },
                    ItemLifecycle::Failed,
                ),
                AdapterInboundEvent::TerminalReason(TurnTerminalReasonCommand {
                    reason: sage_acp_protocol::lifecycle::TurnTerminalReason::HarnessExited,
                    message: Some("The harness exited unexpectedly.".into()),
                    remediation: None,
                    now_millis: 7,
                }),
                AdapterInboundEvent::Turn {
                    state: TurnState::Failed,
                    now_millis: 8,
                },
            ],
        ),
    )
    .unwrap();
    let page = load_chat_page(
        &fixture.connection,
        fixture.owner.project_id,
        fixture.owner.chat_id,
        None,
    )
    .unwrap();
    let actual = serde_json::to_value(page).unwrap();
    let expected: serde_json::Value = serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../src/test/fixtures/nativeTurnFailure.json"
    )))
    .unwrap();
    assert_eq!(
        actual["turns"][0]["terminalReason"],
        expected["turns"][0]["terminalReason"]
    );
    let actual_error = actual["turns"][0]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "error")
        .unwrap();
    let expected_error = &expected["turns"][0]["items"][1];
    for field in [
        "projectId",
        "chatId",
        "sessionId",
        "turnId",
        "runtimeEpoch",
        "origin",
        "kind",
        "lifecycle",
        "payload",
    ] {
        assert_eq!(
            actual_error[field], expected_error[field],
            "native failure field {field}"
        );
    }
    assert!(actual.get("sessionNotices").is_none());
}
