use super::{head_continuation::restarts_after_undo, *};
use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId},
    store::{RestartSessionCommand, restart_undone_session, turn_undo::fixture::Fixture},
};

#[test]
fn a_parked_head_resumes_cut_before_an_undone_turn_or_restarts_without_history() {
    let fixture = Fixture::new();
    fixture.turn("turn-1", 0);
    fixture.turn("turn-2", 1);
    fixture.undo("turn-2", false);
    let parked = SageAgentSessionId::parse("session-a".to_owned()).unwrap();
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: parked.clone(),
        runtime_epoch: RuntimeEpoch::new(2).unwrap(),
    };
    let load = |session: &SageAgentSessionId| {
        load_durable_session(&fixture.database, &fixture.project, &fixture.chat, session).unwrap()
    };
    let mode = |session: &SageAgentSessionId| {
        let rewind = pending_cut(
            &fixture.database,
            &RuntimeOwner {
                session_id: session.clone(),
                ..owner.clone()
            },
        )
        .unwrap();
        activation_mode(load(session), "root".into(), None, rewind).unwrap()
    };
    assert!(!restarts_after_undo(&fixture.database, &load(&parked), &parked).unwrap());
    assert!(matches!(
        mode(&parked),
        ActivationMode::Resume { external_session_id, rewind_at: Some(turn), .. }
            if external_session_id.as_str() == "thread-a" && turn == "00000000-0000-4000-8000-000000000002"
    ));

    fixture.undo("turn-1", false);
    assert!(restarts_after_undo(&fixture.database, &load(&parked), &parked).unwrap());
    let command = RestartSessionCommand {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        expected_projection_revision: fixture.revision(),
        session_id: parked,
        now_millis: 70,
    };
    let restarted = restart_undone_session(&mut fixture.database.connection(), command)
        .unwrap()
        .session_id;
    assert!(matches!(mode(&restarted), ActivationMode::New { .. }));
}
