use super::{rewind, turn_undo};
use sage_acp_protocol::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageTurnId},
    scheduler::RuntimeOwner,
};

pub(crate) mod history;

#[test]
fn repeated_prompts_count_accepted_steers_in_the_target_and_later_turns() {
    let mut history = history::History::new();
    let prompt = crate::acp::transcript_quote::Prompt::new("prompt", "Repeat").unwrap();
    history.turn(prompt.clone(), Some("First steer"));
    history.turn(prompt, Some("Second steer"));
    assert_eq!(history.target(0).unwrap().following_messages, 3);
    assert_eq!(history.target(1).unwrap().following_messages, 1);
}

#[test]
fn legacy_prompts_without_receipts_and_undelivered_prompts_never_become_targets() {
    let mut history = history::History::new();
    let prompt = crate::acp::transcript_quote::Prompt::new("prompt", "Repeat").unwrap();
    history.historical_turn(prompt.clone(), None);
    history.historical_turn(prompt.clone(), None);
    history.unwritten(prompt);
    for i in 0..3 {
        assert!(history.target(i).is_err());
    }
    let page = super::load_chat_page(
        &history.fixture.database.connection(),
        history.fixture.project.clone(),
        history.fixture.chat.clone(),
        None,
    )
    .unwrap();
    assert!(
        page.turns
            .as_slice()
            .iter()
            .all(|turn| !turn.conversation_target)
    );
}

#[test]
fn undelivered_later_prompts_do_not_shift_a_delivered_repeated_target() {
    let mut history = history::History::new();
    let prompt = crate::acp::transcript_quote::Prompt::new("prompt", "Repeat").unwrap();
    history.turn(prompt.clone(), None);
    history.turn(prompt, None);
    history.unwritten(crate::acp::transcript_quote::Prompt::new("prompt", "Unwritten B").unwrap());
    assert_eq!(history.target(0).unwrap().following_messages, 1);
    assert_eq!(history.target(1).unwrap().following_messages, 0);
    assert!(history.target(2).is_err());
    let page = super::load_chat_page(
        &history.fixture.database.connection(),
        history.fixture.project.clone(),
        history.fixture.chat.clone(),
        None,
    )
    .unwrap();
    assert_eq!(
        page.turns
            .as_slice()
            .iter()
            .map(|turn| turn.conversation_target)
            .collect::<Vec<_>>(),
        [false, true, true]
    );
}

#[test]
fn quoted_targets_verify_the_saved_transmitted_prompt() {
    use sha2::{Digest, Sha256};
    let prompt = history::quoted_prompt();
    assert_ne!(prompt.as_str(), prompt.display_text());
    let expected: [u8; 32] = Sha256::digest(prompt.as_str().as_bytes()).into();
    let mut history = history::History::new();
    history.turn(prompt.clone(), None);
    assert_eq!(history.target(0).unwrap().prompt_digest, Some(expected));
}

#[test]
fn file_snapshots_use_the_saved_native_digest_and_legacy_prompts_need_native_identity() {
    use sha2::{Digest, Sha256};
    let prompt = history::file_prompt();
    let expected: [u8; 32] =
        Sha256::digest(prompt.resources.inline(prompt.as_str()).as_bytes()).into();
    let mut history = history::History::new();
    history.turn(prompt.clone(), None);
    assert_eq!(history.target(0).unwrap().prompt_digest, Some(expected));
    let mut historical = history::History::new();
    historical.historical_turn(prompt, None);
    let Err(super::StoreError::RewindUnavailable { reason }) = historical.target(0) else {
        panic!("an unrecoverable native prompt must not be a rewind target");
    };
    assert_eq!(reason, "Older messages can't be rewound.");
    assert_eq!(history.fixture.page_turns().len(), 1);

    let mut correlated = history::History::new();
    correlated.historical_turn(history::file_prompt(), Some("native-turn"));
    let target = correlated.target(0).unwrap();
    assert_eq!(target.native_turn_id.unwrap().as_str(), "native-turn");
    assert_eq!(target.prompt_digest, None);
}

#[test]
fn an_earlier_rewind_atomically_hides_the_exact_suffix_and_survives_restart() {
    let mut history = history::History::new();
    for turn in ["turn-1", "turn-2", "turn-3"] {
        history.historical_turn(
            crate::acp::transcript_quote::Prompt::new("prompt", turn).unwrap(),
            Some(turn),
        );
    }
    let fixture = &history.fixture;
    let owner = RuntimeOwner {
        project_id: fixture.project.clone(),
        chat_id: fixture.chat.clone(),
        session_id: SageAgentSessionId::parse("session-a").unwrap(),
        runtime_epoch: RuntimeEpoch::new(fixture.epoch() as u64).unwrap(),
    };
    let command = crate::acp::turn_undo::stage_turn_undo(
        &fixture.database,
        &owner,
        history.turns[1].clone(),
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
        [history.turns[2].as_str(), history.turns[1].as_str()]
    );
    assert_eq!(
        fixture.page_turns(),
        [
            history.turns[2].as_str(),
            history.turns[1].as_str(),
            history.turns[0].as_str()
        ],
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
    assert_eq!(fixture.page_turns(), [history.turns[0].as_str()]);
    assert_eq!(
        fixture.pending(),
        Some((
            history.turns[1].as_str().into(),
            turn_undo::UndoneThread::Cut
        ))
    );
    let reopened = crate::database::open_database(&fixture.root.join("sage.db")).unwrap();
    assert_eq!(
        turn_undo::pending_thread_rewind(&reopened.connection(), "session-a")
            .unwrap()
            .unwrap()
            .turn_id
            .as_str(),
        history.turns[1].as_str()
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
    let mut history = history::History::new();
    history.historical_turn(
        crate::acp::transcript_quote::Prompt::new("prompt", "Inspect").unwrap(),
        Some("native-1"),
    );
    history.historical_turn(
        crate::acp::transcript_quote::Prompt::new("prompt", "Later").unwrap(),
        Some("native-2"),
    );
    let fixture = &history.fixture;
    let target = || {
        rewind::target(
            &fixture.database.connection(),
            fixture.project.as_str(),
            fixture.chat.as_str(),
            "session-a",
            history.turns[0].as_str(),
        )
    };
    assert_eq!(
        target().unwrap().native_turn_id.unwrap().as_str(),
        "native-1"
    );
    let mut busy_history = history::History::new();
    busy_history.turn(
        crate::acp::transcript_quote::Prompt::new("prompt", "Inspect").unwrap(),
        None,
    );
    assert!(busy_history.target(0).is_ok());
    let busy = &busy_history.fixture;
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
            busy_history.turns[0].as_str()
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
        Some(history.turns[1].as_str()),
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
