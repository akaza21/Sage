use super::TurnUndo;
use crate::acp::{
    bindings::{claude::sdk::CLAUDE_SDK_BINDING_ID, codex::CODEX_BINDING_ID},
    service::AcpService,
    store::turn_undo::fixture::Fixture,
};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageTurnId},
    scheduler::RuntimeOwner,
};

#[test]
fn claude_restores_files_and_codex_rewinds_only_its_own_runtimes_turn() {
    let offered = |binding: &str, session: u64, turn: u64| {
        TurnUndo::of(binding)
            .filter(|undo| undo.offers(session, turn))
            .map(|undo| undo.keeps_files)
    };
    assert_eq!(offered(CLAUDE_SDK_BINDING_ID, 2, 1), Some(false));
    assert_eq!(offered(CODEX_BINDING_ID, 2, 2), Some(true));
    assert_eq!(offered(CODEX_BINDING_ID, 2, 1), None);
    assert_eq!(offered("pi-rpc", 1, 1), None);
}

/// The service starts first: its startup recovery would park a runtime it does
/// not run.
fn running(binding: &str) -> (Fixture, AcpService) {
    let fixture = Fixture::on(binding);
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
fn codex_offers_no_second_undo_in_the_runtime_that_reverted_a_turn() {
    let codex = running(CODEX_BINDING_ID);
    codex.0.turn("turn-1", 0);
    codex.0.turn("turn-2", 1);
    assert_eq!(offer(&codex, "turn-2"), (Some("turn-2".into()), true));
    codex.0.undo("turn-2", true);
    assert_eq!(offer(&codex, "turn-1"), (None, false));

    // Claude Code cuts its thread on the next launch, so it goes on.
    let claude = running(CLAUDE_SDK_BINDING_ID);
    claude.0.turn("turn-1", 0);
    claude.0.turn("turn-2", 1);
    claude.0.undo("turn-2", false);
    assert_eq!(offer(&claude, "turn-1"), (Some("turn-1".into()), true));
}

#[test]
fn a_turn_a_fork_was_taken_through_is_not_offered() {
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
    assert_eq!(offer(&claude, "turn-1"), (None, false));
}
