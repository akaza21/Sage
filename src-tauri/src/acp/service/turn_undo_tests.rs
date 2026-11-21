use crate::acp::{
    bindings::claude::sdk::CLAUDE_SDK_BINDING_ID, service::AcpService,
    store::turn_undo::fixture::Fixture,
};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageTurnId},
    scheduler::RuntimeOwner,
};

/// The service starts first: its startup recovery would park a runtime it does
/// not run.
fn running(binding: &str) -> (Fixture, AcpService) {
    let mut caps: sage_acp_protocol::intent::CapabilitySnapshotV1 =
        serde_json::from_str(crate::acp::store::turn_undo::fixture::CAPABILITIES).unwrap();
    caps.rewind = crate::acp::bindings::claude::sdk::rewind_capability();
    let fixture = Fixture::with_thread(binding, "thread-a", &serde_json::to_string(&caps).unwrap());
    let service = AcpService::new(fixture.database.clone()).unwrap();
    fixture.run();
    (fixture, service)
}

/// The turn the page offers to undo, and whether the control would be let
/// through for `turn`.
fn offer((fixture, service): &(Fixture, AcpService), turn: &str) -> (Option<String>, bool) {
    let mut page = crate::acp::store::load_chat_page(
        &fixture.database.connection(),
        fixture.project.clone(),
        fixture.chat.clone(),
        None,
    )
    .unwrap();
    service.compose_session_surface(&mut page).unwrap();
    let offered = page
        .session_surface
        .and_then(|surface| surface.undo_turn)
        .map(|turn| turn.as_str().to_owned());
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a".to_owned()).unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let allowed = service
        .undoable_turn(&owner, &SageTurnId::parse(turn.to_owned()).unwrap())
        .unwrap()
        .is_some();
    (offered, allowed)
}

#[test]
fn fork_lineage_does_not_withhold_a_capable_conversation_rewind() {
    let claude = running(CLAUDE_SDK_BINDING_ID);
    let fixture = &claude.0;
    fixture.turn("turn-1", 0);
    assert_eq!(offer(&claude, "turn-1"), (Some("turn-1".into()), true));
    fixture
        .database
        .connection()
        .execute_batch(&format!(
            "INSERT INTO chats(id,project_id,ordinal,title,created_at,updated_at)
               VALUES('chat-fork','{project}',9,'Fork',1,1);
             INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,
               parent_chat_id,source_session_id,source_turn_id,relation,external_operation_id,
               created_at)
             VALUES('{project}','chat-fork','{project}','{chat}','session-a','turn-1',
               'nativeFork',NULL,1);",
            project = fixture.project.as_str(),
            chat = fixture.chat.as_str(),
        ))
        .unwrap();
    assert_eq!(offer(&claude, "turn-1"), (Some("turn-1".into()), true));
}
