use super::*;
use crate::acp::{
    contract::{
        RequiredText, RuntimeEpoch, SafeText, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    intent::{ExternalId, NormalizedPayload, ToolCategory},
    runtime::permissions::PermissionIntent,
};

#[test]
fn browser_local_permission_is_durable_and_projects_as_the_existing_card() {
    let mut connection = super::extended_tests::setup();
    let started = start_session(
        &mut connection,
        super::extended_tests::start(0, "start-browser-permission"),
    )
    .unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET state='active',binding_digest=?2 WHERE id=?1",
            rusqlite::params![started.session_id.as_str(), "a".repeat(64)],
        )
        .unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'running',NULL,1,NULL,NULL)",[started.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-browser','project-one','chat-one',?1,1,0,'turn-intent','running','completeBytes',0,1,1,NULL)",[started.session_id.as_str()]).unwrap();
    let body = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        user_input: None,
        category: ToolCategory::Mcp,
        summary: SafeText::new("summary", "Browser snapshotPage on https://example.com").unwrap(),
        offered_option_kinds: None,
        reason: None,
    })
    .unwrap();
    let digest = super::facts::hex_digest(body.as_bytes());
    let intent = PermissionIntent {
        request_id: SageServerRequestId::parse("browser-request-one").unwrap(),
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: started.session_id.clone(),
        turn_id: SageTurnId::parse("turn-browser").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        binding_digest: "a".repeat(64),
        external_request_id: ExternalId::new("externalRequestId", "browser-external-one").unwrap(),
        kind: RequiredText::new("kind", "browserPermission").unwrap(),
        payload_digest: "b".repeat(64),
        normalized_payload_digest: digest,
        normalized_payload_json: body,
    };
    receive_permission_request(&mut connection, &intent, 2).unwrap();
    let page = load_chat_page(
        &connection,
        intent.project_id.clone(),
        intent.chat_id.clone(),
        None,
    )
    .unwrap();
    let item = &page.turns.as_slice()[0].items.as_slice()[0];
    match &item.content {
        crate::acp::projection::ItemContent::PermissionRequest { label, summary, .. } => {
            assert_eq!(label.as_str(), "Browser permission");
            assert!(summary.as_str().contains("https://example.com"));
        }
        _ => panic!("browser request must use existing permission projection"),
    }
}
