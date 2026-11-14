use super::{UndoneThread, fixture::Fixture};
use crate::acp::{
    store::{
        MutationDisposition, PrepareTurnCommand, RestartSessionCommand, StoreError, load_chat_page,
        prepare_turn, restart_undone_session, unstarted_continuation,
    },
    transcript_quote::Prompt,
};
use sage_acp_protocol::contract::{
    ClientIntentId, ProjectGeneration, RuntimeEpoch, SageAgentSessionId,
};

#[test]
fn an_undone_turn_leaves_the_page_and_undo_moves_to_the_turn_before() {
    let fixture = Fixture::new();
    fixture.turn("turn-1", 0);
    fixture.turn("turn-2", 1);
    assert!(fixture.undoable("turn-2"));
    assert!(!fixture.undoable("turn-1"));
    let before = fixture.revision();

    let undone = fixture.undo("turn-2", false);
    assert_eq!(undone.thread, UndoneThread::Cut);
    assert_eq!(fixture.page_turns(), ["turn-1"]);
    assert_eq!(undone.published, Some(fixture.revision()));
    assert!(fixture.revision() > before, "the page is published again");
    assert!(!fixture.undoable("turn-2"));
    assert!(fixture.undoable("turn-1"));

    let published = fixture.revision();
    let again = fixture.undo("turn-2", false);
    assert_eq!((again.thread, again.published), (UndoneThread::Cut, None));
    assert_eq!(fixture.revision(), published);
    let facts: i64 = fixture
        .database
        .connection()
        .query_row(
            "SELECT COUNT(*) FROM session_facts WHERE kind='turnUndone'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(facts, 1);
}

#[test]
fn a_cut_waits_until_a_newer_turn_runs_and_the_latest_undo_wins() {
    let fixture = Fixture::new();
    for (ordinal, turn) in ["turn-1", "turn-2", "turn-3"].into_iter().enumerate() {
        fixture.turn(turn, ordinal as i64);
    }
    assert_eq!(fixture.pending(), None);
    fixture.undo("turn-3", false);
    assert_eq!(
        fixture.pending(),
        Some(("turn-3".into(), UndoneThread::Cut))
    );
    fixture.undo("turn-2", false);
    assert_eq!(
        fixture.pending(),
        Some(("turn-2".into(), UndoneThread::Cut))
    );
    assert_eq!(fixture.page_turns(), ["turn-1"]);
    fixture.relaunch();
    assert_eq!(
        fixture.pending(),
        Some(("turn-2".into(), UndoneThread::Cut))
    );
    fixture.turn("turn-4", 3);
    assert_eq!(fixture.pending(), None);
}

#[test]
fn the_runtime_that_holds_an_undone_turn_takes_no_prompt_and_a_raced_prompt_refuses_the_undo() {
    let prepare = |fixture: &Fixture| {
        let command = PrepareTurnCommand {
            project_id: fixture.project.clone(),
            project_generation: ProjectGeneration::new(1).unwrap(),
            chat_id: fixture.chat.clone(),
            session_id: SageAgentSessionId::parse("session-a".to_owned()).unwrap(),
            runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
            expected_projection_revision: fixture.revision(),
            client_intent_id: ClientIntentId::new(),
            prompt: Prompt::new("prompt", "next").unwrap(),
            now_millis: 60,
        };
        prepare_turn(&mut fixture.database.connection(), command)
    };
    let fixture = Fixture::new();
    fixture.run();
    fixture.turn("turn-1", 0);
    fixture.turn("turn-2", 1);
    fixture.undo("turn-2", false);
    assert!(matches!(
        prepare(&fixture),
        Err(StoreError::Conflict {
            resource: "sessionEpoch"
        })
    ));

    let raced = Fixture::new();
    raced.run();
    raced.turn("turn-1", 0);
    raced.turn("turn-2", 1);
    prepare(&raced).unwrap();
    assert!(raced.try_undo("turn-2", false).is_err());
    assert_eq!(raced.pending(), None);
}

#[test]
fn undoing_the_threads_first_turn_restarts_and_a_harness_revert_needs_nothing() {
    let fixture = Fixture::new();
    fixture.turn("turn-1", 0);
    assert_eq!(fixture.undo("turn-1", false).thread, UndoneThread::Restart);
    assert_eq!(
        fixture.pending(),
        Some(("turn-1".into(), UndoneThread::Restart))
    );
    assert!(fixture.page_turns().is_empty());

    let reverted = Fixture::new();
    reverted.turn("turn-1", 0);
    reverted.turn("turn-2", 1);
    assert_eq!(reverted.undo("turn-2", true).thread, UndoneThread::Reverted);
    assert_eq!(reverted.page_turns(), ["turn-1"]);
    assert_eq!(reverted.pending(), None);
}

#[test]
fn a_restart_replaces_the_parked_head_with_a_fresh_one_on_the_same_binding() {
    let fixture = Fixture::new();
    fixture.turn("turn-1", 0);
    let restart = || {
        let command = RestartSessionCommand {
            project_id: fixture.project.clone(),
            chat_id: fixture.chat.clone(),
            expected_projection_revision: fixture.revision(),
            session_id: SageAgentSessionId::parse("session-a".to_owned()).unwrap(),
            now_millis: 60,
        };
        restart_undone_session(&mut fixture.database.connection(), command)
    };
    assert!(restart().is_err(), "no restart is pending");
    fixture.undo("turn-1", false);

    let restarted = restart().unwrap();
    assert_eq!(restarted.disposition, MutationDisposition::Applied);
    let row = |id: &str| -> (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) {
        fixture
            .database
            .connection()
            .query_row(
                "SELECT state,binding_id,model_id,external_session_id,resumes_session_id
                 FROM agent_sessions WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .unwrap()
    };
    assert_eq!(
        row(restarted.session_id.as_str()),
        (
            "created".into(),
            "claude-code-sdk".into(),
            Some("opus".into()),
            None,
            None
        )
    );
    assert_eq!(row("session-a").0, "closed");
    assert_eq!(
        crate::acp::store::preceding_session(&fixture.database.connection(), &restarted.session_id)
            .unwrap()
            .as_ref()
            .map(SageAgentSessionId::as_str),
        Some("session-a")
    );
    let page = load_chat_page(
        &fixture.database.connection(),
        fixture.project.clone(),
        fixture.chat.clone(),
        None,
    )
    .unwrap();
    assert_eq!(page.current_session_id, Some(restarted.session_id.clone()));

    let retried = restart().unwrap();
    assert_eq!(retried.session_id, restarted.session_id);
    assert_eq!(retried.disposition, MutationDisposition::Duplicate);
    let found = unstarted_continuation(
        &fixture.database.connection(),
        fixture.project.as_str(),
        fixture.chat.as_str(),
        "session-a",
    )
    .unwrap()
    .map(|head| head.session_id);
    assert_eq!(found, Some(restarted.session_id));
}
