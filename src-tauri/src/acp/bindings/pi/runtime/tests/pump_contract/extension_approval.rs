use super::*;
use crate::acp::{bindings::pi::bridge::Refusal, store::load_chat_page};
use sage_acp_protocol::{contract::SageTurnId, permissions::PermissionDecision};

fn adapter() -> ObservedPi {
    let mut adapter = ready_adapter();
    adapter.inner.enable_bridge(
        sage_workspace::Workspace::open(
            "project",
            "root-pi",
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")),
        )
        .unwrap(),
    );
    adapter
        .inner
        .ingest(&frame(super::super::bridge_ready::announcement()), &Mapper)
        .unwrap();
    adapter
}

async fn prepare(pump: &mut Pump, args: Value) -> Value {
    prepare_as(pump, "extension-1", "extension-approval", args).await
}

async fn prepare_as(pump: &mut Pump, tool_id: &str, id: &str, args: Value) -> Value {
    push(
        pump,
        json!({"type":"tool_execution_start",
        "toolCallId":tool_id,"toolName":"fixture_echo","args":args}),
    )
    .await;
    let request = json!({"type":"extension_ui_request","method":"input","id":id,
        "title":"sage.pi.prepare.v1","placeholder":json!({"version":1,"toolCallId":tool_id,
        "tool":"fixture_echo","kind":"extension","args":args}).to_string()});
    push(pump, request.clone()).await;
    request
}

async fn push(pump: &mut Pump, value: Value) {
    for chunk in line(value).chunks(8192) {
        pump.push_stdout_chunk(chunk, 4).await.unwrap();
    }
}

#[tokio::test]
async fn abandoned_extension_approval_is_cancelled_before_withdrawal_reaches_pi() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(adapter());
    start(&mut pump).await;
    prepare(&mut pump, json!({"text":"nested-held"})).await;
    let request_id: String = fixture
        .database
        .connection()
        .query_row("SELECT id FROM server_requests", [], |row| row.get(0))
        .unwrap();
    let request_id = SageServerRequestId::parse(request_id).unwrap();
    push(
        &mut pump,
        json!({"type":"extension_ui_request","method":"input","id":"withdraw-held",
            "title":"sage.pi.withdraw.v1","placeholder":"extension-1"}),
    )
    .await;
    {
        let connection = fixture.database.connection();
        let state: String = connection
            .query_row("SELECT state FROM server_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(state, "cancelled");
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        assert!(
            page.turns.as_slice()[0]
                .items
                .as_slice()
                .iter()
                .any(|item| {
                    matches!(
                        &item.content,
                        crate::acp::projection::ItemContent::PermissionRequest { state, .. }
                            if *state == sage_acp_protocol::lifecycle::ServerRequestState::Cancelled
                    )
                })
        );
    }
    // A user decision already in flight cannot revive the withdrawn approval.
    pump.resolve_permission(&request_id, PermissionDecision::AllowOnce, 5)
        .await
        .unwrap();
    let mut transport = Capture::default();
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(
        transport.0,
        [json!({"type":"extension_ui_response","id":"withdraw-held",
            "value":Refusal::Withdrawn.message()})]
    );
    assert!(!pump.settle_written_permission(6).await.unwrap());
    pump.accept_delivered_answers(6).await.unwrap();
    push(
        &mut pump,
        json!({"type":"tool_execution_end","toolCallId":"extension-1",
            "toolName":"fixture_echo","isError":true,
            "result":{"content":[{"type":"text","text":Refusal::Withdrawn.message()}]}}),
    )
    .await;
    // The next real prepare gets a fresh approval slot in the same turn.
    prepare_as(
        &mut pump,
        "extension-2",
        "extension-next",
        json!({"text":"nested-after-timeout"}),
    )
    .await;
    let states: Vec<String> = fixture
        .database
        .connection()
        .prepare("SELECT state FROM server_requests ORDER BY rowid")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert_eq!(states, ["cancelled", "received"]);
    push(
        &mut pump,
        json!({"type":"extension_ui_request","method":"input","id":"withdraw-next",
            "title":"sage.pi.withdraw.v1","placeholder":"extension-2"}),
    )
    .await;
    pump.flush_one(&mut transport).unwrap();
    assert_eq!(
        transport.0[1],
        json!({"type":"extension_ui_response","id":"withdraw-next",
            "value":Refusal::Withdrawn.message()})
    );
    pump.accept_delivered_answers(7).await.unwrap();
    settle(&mut pump).await;
    assert!(!pump.teardown_required());
    fixture.finish().await;
}

#[tokio::test]
async fn extension_hidden_formats_are_visible_after_persistence_and_projection() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(adapter());
    start(&mut pump).await;
    let name = "fixture_\u{200b}echo";
    let args = json!({"path":"invoice\u{202e}txt.exe","apiKey":"private\u{202e}key"});
    push(
        &mut pump,
        json!({"type":"tool_execution_start","toolCallId":"extension-1",
            "toolName":name,"args":args}),
    )
    .await;
    push(
        &mut pump,
        json!({"type":"extension_ui_request","method":"input","id":"extension-approval",
            "title":"sage.pi.prepare.v1","placeholder":json!({"version":1,
                "toolCallId":"extension-1","tool":name,"kind":"extension",
                "label":"Invoice\u{2066} tool\u{2069}","args":args}).to_string()}),
    )
    .await;
    {
        let connection = fixture.database.connection();
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        let summary = page.turns.as_slice()[0]
            .items
            .as_slice()
            .iter()
            .find_map(|item| match &item.content {
                crate::acp::projection::ItemContent::PermissionRequest { summary, .. } => {
                    Some(summary.as_str())
                }
                _ => None,
            })
            .expect("persisted approval card");
        assert_eq!(
            summary,
            "Allow extension tool fixture_\\u{200B}echo (Invoice\\u{2066} tool\\u{2069})?\n\
             {\"path\":\"invoice\\u{202E}txt.exe\",[redacted]}"
        );
    }
    fixture.finish().await;
}

#[tokio::test]
async fn denied_extension_credentials_are_redacted_in_both_sqlite_records_and_projection() {
    let fixture = Fixture::new();
    let mut pump = fixture.pump(adapter());
    start(&mut pump).await;
    let request = prepare(
        &mut pump,
        json!({"headers":{"Authorization":"Bearer private-header"},
        "apiKey":"private-key", "text":"visible argument", "path":"/tmp/report.txt", "template":"use `literal`"}),
    )
    .await;
    let request_id: String = fixture
        .database
        .connection()
        .query_row("SELECT id FROM server_requests", [], |row| row.get(0))
        .unwrap();
    let request_id = SageServerRequestId::parse(request_id).unwrap();
    crate::acp::store::apply_request_state(
        &mut fixture.database.connection(),
        crate::acp::store::RequestStateCommand {
            project_id: owner().project_id,
            chat_id: owner().chat_id,
            session_id: owner().session_id,
            turn_id: SageTurnId::parse("turn-pump-0").unwrap(),
            runtime_epoch: owner().runtime_epoch,
            request_id: request_id.clone(),
            state: sage_acp_protocol::lifecycle::ServerRequestState::DeclinePending,
            now_millis: 5,
        },
    )
    .unwrap();
    pump.resolve_permission(&request_id, PermissionDecision::Deny, 6)
        .await
        .unwrap();
    pump.flush_one(&mut Capture::default()).unwrap();
    assert!(pump.settle_written_permission(7).await.unwrap());
    {
        let connection = fixture.database.connection();
        let state: String = connection
            .query_row("SELECT state FROM server_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(state, "resolvedDeclined");
        for sql in [
            "SELECT payload_json FROM items WHERE kind='permissionRequest'",
            "SELECT normalized_payload_json FROM server_requests",
        ] {
            let payload: String = connection.query_row(sql, [], |row| row.get(0)).unwrap();
            assert!(payload.contains("visible argument"));
            assert!(payload.contains("/tmp/report.txt") && payload.contains("use `literal`"));
            assert!(payload.contains("[redacted]"));
            assert!(
                !payload.contains("private-header") && !payload.contains("private-key"),
                "{payload}"
            );
        }
        let digest: String = connection
            .query_row("SELECT payload_digest FROM server_requests", [], |row| {
                row.get(0)
            })
            .unwrap();
        let expected = crate::acp::transport::request_payload_digest(&frame(request)).unwrap();
        assert_eq!(
            digest,
            expected
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
        );
        let page = load_chat_page(&connection, owner().project_id, owner().chat_id, None).unwrap();
        let projection = serde_json::to_string(&page).unwrap();
        assert!(projection.contains("visible argument") && projection.contains("[redacted]"));
        assert!(projection.contains("/tmp/report.txt") && projection.contains("use `literal`"));
        assert!(!projection.contains("private-header") && !projection.contains("private-key"));
    }
    fixture.finish().await;
}

#[tokio::test]
async fn serialized_extension_permissions_refuse_escaped_and_over_limit_inputs_cleanly() {
    for (args, admitted) in [
        (json!({"text":"\"".repeat(22_000)}), false),
        (json!({"text":"x".repeat(65_300)}), true),
        (json!({"text":"x".repeat(65_400)}), false),
        (json!({"text":"\u{202e}".repeat(9000)}), false),
        (
            json!({"apiKey":"x".repeat(9000),"path":"/tmp/report.txt","text":"operation after credential"}),
            false,
        ),
        (
            json!({"command":format!("curl --token \"{}\" && touch visible", "x".repeat(9000))}),
            false,
        ),
        (
            json!({"key":format!("-----BEGIN PRIVATE KEY-----\n{}\n-----END PRIVATE KEY-----", "x".repeat(2000)),
                "path":"/tmp/report.txt", "text":"operation after private key"}),
            false,
        ),
    ] {
        let fixture = Fixture::new();
        let mut pump = fixture.pump(adapter());
        start(&mut pump).await;
        prepare(&mut pump, args).await;
        assert!(!pump.teardown_required());
        let requests: i64 = fixture
            .database
            .connection()
            .query_row("SELECT count(*) FROM server_requests", [], |row| row.get(0))
            .unwrap();
        assert_eq!(requests, i64::from(admitted));
        let mut transport = Capture::default();
        pump.flush_one(&mut transport).unwrap();
        if admitted {
            assert!(transport.0.is_empty());
        } else {
            assert_eq!(
                transport.0,
                [
                    json!({"type":"extension_ui_response","id":"extension-approval","value":Refusal::TooLarge.message()})
                ]
            );
        }
        fixture.finish().await;
    }
}
