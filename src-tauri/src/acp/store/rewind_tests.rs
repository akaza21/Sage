use super::{
    rewind,
    turn_undo::{self, fixture::Fixture},
};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageTurnId},
    scheduler::RuntimeOwner,
};

#[test]
fn an_earlier_rewind_atomically_hides_the_exact_suffix_and_survives_restart() {
    let fixture = Fixture::new();
    fixture.run();
    for (ordinal, turn) in ["turn-1", "turn-2", "turn-3"].into_iter().enumerate() {
        fixture.turn(turn, ordinal as i64);
        fixture.prompt(turn, turn);
    }
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let command = crate::acp::turn_undo::stage_turn_undo(
        &fixture.database,
        &owner,
        SageTurnId::parse("turn-2").unwrap(),
        false,
        true,
        || Ok::<_, String>(Some("00000000-0000-4000-8000-000000000002".into())),
    )
    .unwrap();
    assert_eq!(
        command
            .rewind_turns
            .iter()
            .map(SageTurnId::as_str)
            .collect::<Vec<_>>(),
        ["turn-3", "turn-2"]
    );
    assert_eq!(
        fixture.page_turns(),
        ["turn-3", "turn-2", "turn-1"],
        "preparation publishes no partial suffix"
    );
    assert!(
        crate::acp::store::turn_undo::intent::ensure_settled(
            &fixture.database.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str()
        )
        .is_err()
    );
    turn_undo::record_turn_undone(&mut fixture.database.connection(), &command).unwrap();
    assert_eq!(fixture.page_turns(), ["turn-1"]);
    assert_eq!(
        fixture.pending(),
        Some(("turn-2".into(), turn_undo::UndoneThread::Cut))
    );
    let reopened = crate::database::open_database(&fixture.root.join("sage.db")).unwrap();
    assert_eq!(
        turn_undo::pending_thread_rewind(&reopened.connection(), "session-a")
            .unwrap()
            .unwrap()
            .turn_id
            .as_str(),
        "turn-2"
    );
    assert!(
        turn_undo::record_turn_undone(&mut reopened.connection(), &command)
            .unwrap()
            .published
            .is_none()
    );
}

#[test]
fn compaction_and_a_busy_turn_refuse_older_targets_before_native_effects() {
    let fixture = Fixture::new();
    fixture.run();
    fixture.turn("turn-1", 0);
    fixture.prompt("turn-1", "Inspect");
    fixture.turn("turn-2", 1);
    fixture.prompt("turn-2", "Later");
    let target = || {
        rewind::target(
            &fixture.database.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str(),
            "session-a",
            "turn-1",
        )
    };
    assert_eq!(target().unwrap().following_messages, 1);
    let busy = Fixture::new();
    busy.run();
    busy.turn("turn-1", 0);
    busy.prompt("turn-1", "Inspect");
    let queued = super::PrepareTurnCommand {
        project_id: busy.project.clone(),
        chat_id: busy.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(busy.epoch() as u64).unwrap(),
        project_generation: sage_acp_protocol::contract::ProjectGeneration::new(1).unwrap(),
        expected_projection_revision: busy.revision(),
        client_intent_id: sage_acp_protocol::contract::ClientIntentId::new(),
        prompt: crate::acp::transcript_quote::Prompt::new("prompt", "Next question").unwrap(),
        now_millis: 50,
    };
    super::prepare_turn(&mut busy.database.connection(), queued).unwrap();
    assert!(
        rewind::target(
            &busy.database.connection(),
            busy.project.as_str(),
            busy.chat.as_str(),
            "session-a",
            "turn-1"
        )
        .is_err()
    );
    let revision = fixture.revision();
    let mut connection = fixture.database.connection();
    let tx = connection.transaction().unwrap();
    super::facts::append_fact_payload(
        &tx,
        fixture.project.as_str(),
        fixture.chat.as_str(),
        "session-a",
        Some("turn-2"),
        "sessionActivity",
        r#"{"i":"compaction","k":"compaction","l":"Compacted","s":"completed"}"#,
        revision,
        50,
    )
    .unwrap();
    tx.commit().unwrap();
    drop(connection);
    assert!(target().is_err());
}
