// Derived from pingdotgg/t3code (commit 08a463e), Copyright (c) 2026 T3 Tools Inc., MIT; see THIRD-PARTY-NOTICES.md.
use super::*;
use crate::acp::{intent::BindingId, store::session_replace::*};

pub(super) fn setup() -> rusqlite::Connection {
    let opened = crate::acp::migration::open_or_migrate(std::path::Path::new(":memory:")).unwrap();
    let (connection, _) = opened.into_parts();
    connection.execute_batch("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','P',1,1,1);
        INSERT INTO chats(id,project_id,title,ordinal,projection_revision,created_at,updated_at) VALUES('chat','project','Chat',0,0,1,1);
        INSERT INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project',1,0,0,0,0,0,0);
        UPDATE acp_store_usage SET chat_count=1 WHERE singleton=1;").unwrap();
    connection
}

pub(super) fn command(
    connection: &rusqlite::Connection,
    intent: &str,
    binding: &str,
) -> ReplaceSessionCommand {
    use rusqlite::OptionalExtension;
    ReplaceSessionCommand {
        confirm_unknown_delivery: false,
        start: StartSessionCommand {
            project_id: SageProjectId::parse("project").unwrap(),
            chat_id: SageChatId::parse("chat").unwrap(),
            expected_projection_revision: ProjectionRevision::new(
                connection
                    .query_row(
                        "SELECT projection_revision FROM chats WHERE id='chat'",
                        [],
                        |row| row.get::<_, i64>(0).map(|value| value as u64),
                    )
                    .unwrap(),
            )
            .unwrap(),
            client_intent_id: ClientIntentId::parse(intent).unwrap(),
            binding_id: BindingId::new("bindingId", binding).unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: None,
            model_id: None,
            now_millis: 2,
        },
        expected_head: connection
            .query_row(
                "SELECT session_id FROM chat_session_heads WHERE chat_id='chat'",
                [],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .unwrap()
            .map(|id| SageAgentSessionId::parse(id).unwrap()),
    }
}

#[test]
fn replacement_is_atomic_and_intent_idempotent() {
    let mut connection = setup();
    let first_command = command(&connection, "first", "a");
    let first = replace_session(&mut connection, first_command).unwrap();
    let next = command(&connection, "next", "b");
    connection.execute_batch("CREATE TRIGGER fail_replace BEFORE INSERT ON acp_session_transitions WHEN NEW.client_intent_id='next' BEGIN SELECT RAISE(ABORT,'fail'); END;").unwrap();
    assert!(replace_session(&mut connection, next.clone()).is_err());
    assert_eq!(head(&connection), first.session_id.as_str());
    connection
        .execute_batch("DROP TRIGGER fail_replace")
        .unwrap();
    let replaced = replace_session(&mut connection, next.clone()).unwrap();
    assert_ne!(first.session_id, replaced.session_id);
    assert_eq!(head(&connection), replaced.session_id.as_str());
    assert_eq!(
        replace_session(&mut connection, next.clone())
            .unwrap()
            .session_id,
        replaced.session_id
    );
    let mut conflict = next;
    conflict.start.binding_id = BindingId::new("bindingId", "c").unwrap();
    assert!(replace_session(&mut connection, conflict).is_err());
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        2
    );
}

fn head(connection: &rusqlite::Connection) -> String {
    connection
        .query_row(
            "SELECT session_id FROM chat_session_heads WHERE chat_id='chat'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

#[test]
fn replacement_rejects_busy_stale_and_capacity() {
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    replace_session(&mut connection, first).unwrap();
    let next = command(&connection, "next", "b");
    let mut stale = next.clone();
    stale.start.expected_projection_revision = ProjectionRevision::ZERO;
    assert!(replace_session(&mut connection, stale).is_err());
    connection
        .execute(
            "UPDATE agent_sessions SET state='starting' WHERE id=?1",
            [head(&connection)],
        )
        .unwrap();
    assert!(replace_session(&mut connection, next.clone()).is_err());
    let mut connection = setup();
    let first = command(&connection, "capacity-first", "a");
    replace_session(&mut connection, first).unwrap();
    let next = command(&connection, "capacity-next", "b");
    connection
        .execute(
            "UPDATE acp_store_usage SET session_count=?1 WHERE singleton=1",
            [crate::acp::limits::LIMITS.durable.sessions_global as i64],
        )
        .unwrap();
    assert!(matches!(
        replace_session(&mut connection, next),
        Err(StoreError::Capacity { .. })
    ));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_sessions", [], |row| row
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
}

#[test]
fn late_old_epoch_cannot_mutate_new_head() {
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    let old = replace_session(&mut connection, first).unwrap();
    historical_turn(&connection, &old.session_id);
    let next = command(&connection, "next", "b");
    let new = replace_session(&mut connection, next).unwrap();
    let revision = new.projection_revision;
    assert!(
        activate_session(
            &mut connection,
            ActivateSessionCommand {
                project_id: SageProjectId::parse("project").unwrap(),
                chat_id: SageChatId::parse("chat").unwrap(),
                expected_projection_revision: revision,
                client_intent_id: ClientIntentId::parse("late").unwrap(),
                session_id: old.session_id.clone(),
                runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                artifact_digest: "d".repeat(64),
                now_millis: 3,
            }
        )
        .is_err()
    );
    let late = AdapterInboundBatchCommand::new(
        AdapterStoreOwner {
            project_id: SageProjectId::parse("project").unwrap(),
            chat_id: SageChatId::parse("chat").unwrap(),
            session_id: old.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: SageTurnId::parse("history-turn").unwrap(),
            binding_digest: "a".repeat(64),
            external_session_id: crate::acp::intent::ExternalId::new("native", "native-old")
                .unwrap(),
        },
        128,
        128,
        vec![AdapterInboundEvent::Turn {
            state: TurnState::Running,
            now_millis: 4,
        }],
    )
    .unwrap();
    assert!(matches!(
        apply_adapter_inbound_batch(&mut connection, late),
        Err(StoreError::Conflict {
            resource: "adapterBatchOwner"
        })
    ));
    assert_eq!(head(&connection), new.session_id.as_str());
    assert_eq!(
        connection
            .query_row("SELECT projection_revision FROM chats", [], |row| row
                .get::<_, i64>(0)
                .map(|value| value as u64))
            .unwrap(),
        revision.get()
    );
}

#[test]
fn reused_transition_intent_conflicts_on_changed_payload() {
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    replace_session(&mut connection, first).unwrap();
    let reused = command(&connection, "reuse", "a");
    retain_current(&mut connection, &reused).unwrap();
    assert!(lookup(&connection, &reused).unwrap().is_some());
    let mut changed = reused;
    changed.start.binding_id = BindingId::new("bindingId", "b").unwrap();
    assert!(matches!(
        replace_session(&mut connection, changed),
        Err(StoreError::Conflict {
            resource: "transitionIntent"
        })
    ));
}

#[test]
fn resume_refusal_creates_fresh_handoff() {
    use crate::acp::service::session_transition::{
        TransitionDecision, TransitionInput, decide_transition,
    };
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    let old = replace_session(&mut connection, first).unwrap();
    historical_turn(&connection, &old.session_id);
    let mut transition = TransitionInput::live();
    transition.cold = true;
    transition.resume_refused = true;
    assert_eq!(decide_transition(&transition), TransitionDecision::Fresh);
    let next = command(&connection, "refused", "a");
    let replacement = replace_session(&mut connection, next).unwrap();
    assert_ne!(old.session_id, replacement.session_id);
    let summaries = super::context_handoff::summaries(&connection, "project", "chat").unwrap();
    assert_eq!(summaries.len(), 1);
    assert_eq!(summaries[0].target_session, replacement.session_id.as_str());
    assert_eq!(
        summaries[0].state,
        crate::acp::context_handoff::HandoffState::Prepared
    );
}

#[test]
fn replacement_accepts_completed_active_owner_after_zero_process() {
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    let old = replace_session(&mut connection, first).unwrap();
    historical_turn(&connection, &old.session_id);
    connection
        .execute(
            "UPDATE agent_sessions SET state='active' WHERE id=?1",
            [old.session_id.as_str()],
        )
        .unwrap();
    let next = command(&connection, "next", "b");
    let new = replace_session(&mut connection, next).unwrap();
    assert_ne!(old.session_id, new.session_id);
    assert_eq!(head(&connection), new.session_id.as_str());
    assert_eq!(
        connection
            .query_row(
                "SELECT state FROM agent_sessions WHERE id=?1",
                [old.session_id.as_str()],
                |row| row.get::<_, String>(0)
            )
            .unwrap(),
        "closed"
    );
}

fn historical_turn(connection: &rusqlite::Connection, session: &SageAgentSessionId) {
    connection
        .execute(
            "UPDATE agent_sessions SET state='starting' WHERE id=?1",
            [session.as_str()],
        )
        .unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET state='ready',external_session_id='native-old',capabilities_json=?2 WHERE id=?1",
            rusqlite::params![session.as_str(), super::extended_tests::capabilities()],
        )
        .unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at,terminal_at,exit_kind) VALUES('project','chat',?1,1,'exited',1,2,'parked')",[session.as_str()]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at,terminal_at) VALUES('history-turn','project','chat',?1,1,0,'history-intent','completed','correlated',1,2,2)",[session.as_str()]).unwrap();
    let payload = r#"{"kind":"text","payload":{"role":"user","text":"first request"}}"#;
    connection.execute("INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,created_at,updated_at,terminal_at) VALUES('history-item','project','chat',?1,'history-turn',1,0,'primary','text','completed',?2,?3,?4,1,2,2)",
        rusqlite::params![session.as_str(),payload,payload.len() as i64,super::facts::hex_digest(payload.as_bytes())]).unwrap();
}

#[test]
fn inline_handoff_is_skipped_on_later_prompt() {
    let (mut connection, mut prompt) = frozen_handoff();
    assert!(prompt.prompt.as_str().contains("first request"));
    record_handoff_delivery(&mut connection, &prompt, DeliveryState::Correlated);
    prompt.prompt = crate::acp::intent::Prompt::new("prompt", "later request").unwrap();
    super::context_handoff::freeze_prompt(&mut connection, &mut prompt).unwrap();
    assert_eq!(prompt.prompt.as_str(), "later request");
    assert_eq!(
        super::context_handoff::summaries(&connection, "project", "chat").unwrap()[0].state,
        crate::acp::context_handoff::HandoffState::Inline
    );
}

#[test]
fn uncorrelated_terminal_write_marks_handoff_unknown() {
    let (mut connection, prompt) = frozen_handoff();
    let mut mutation = TurnMutationCommand {
        project_id: prompt.project_id.clone(),
        chat_id: prompt.chat_id.clone(),
        session_id: prompt.session_id.clone(),
        runtime_epoch: prompt.runtime_epoch,
        turn_id: SageTurnId::parse("handoff-turn").unwrap(),
        expected_revision: EntityRevision::ZERO,
        now_millis: 4,
    };
    let sending = super::send::transition_turn(
        &mut connection,
        mutation.clone(),
        TurnState::Sending,
        DeliveryState::Writing,
        "sendStarted",
    )
    .unwrap();
    assert_eq!(
        super::context_handoff::summaries(&connection, "project", "chat").unwrap()[0].state,
        crate::acp::context_handoff::HandoffState::Pending
    );
    mutation.expected_revision = sending.revision;
    let failed = super::send::transition_turn(
        &mut connection,
        mutation,
        TurnState::Failed,
        DeliveryState::Writing,
        "turnTerminal",
    )
    .unwrap();
    assert_eq!(failed.state, TurnState::Failed);
    assert_eq!(failed.delivery_state, DeliveryState::Writing);
    assert_eq!(
        super::context_handoff::summaries(&connection, "project", "chat").unwrap()[0].state,
        crate::acp::context_handoff::HandoffState::Unknown
    );
}

#[test]
fn larger_retry_draft_cannot_exceed_the_frozen_plan_budget() {
    let (mut connection, mut prompt) = frozen_handoff();
    let before: String = connection
        .query_row("SELECT plan_json FROM acp_context_handoffs", [], |row| {
            row.get(0)
        })
        .unwrap();
    prompt.prompt = crate::acp::intent::Prompt::new("prompt", "x".repeat(99_000)).unwrap();
    assert!(matches!(
        super::context_handoff::freeze_prompt(&mut connection, &mut prompt),
        Err(StoreError::HandoffBudgetExceeded)
    ));
    let after: String = connection
        .query_row("SELECT plan_json FROM acp_context_handoffs", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(before, after);
}

#[test]
fn recovered_unknown_handoff_requires_confirmation_without_a_head() {
    let (mut connection, prompt) = frozen_handoff();
    record_handoff_delivery(&mut connection, &prompt, DeliveryState::Writing);
    let transaction = connection.transaction().unwrap();
    crate::acp::context_handoff::delivery::recover_epoch(
        &transaction,
        prompt.session_id.as_str(),
        1,
        prompt.expected_projection_revision,
        5,
    )
    .unwrap();
    transaction
        .execute("DELETE FROM chat_session_heads WHERE chat_id='chat'", [])
        .unwrap();
    transaction.execute("UPDATE session_runtime_epochs SET state='failed',terminal_at=5,exit_kind='restartRecovery' WHERE session_id=?1", [prompt.session_id.as_str()]).unwrap();
    transaction
        .execute(
            "UPDATE turns SET state='sending',delivery_state='writing' WHERE id='handoff-turn'",
            [],
        )
        .unwrap();
    transaction.execute("UPDATE turns SET state='deliveryUnknown',delivery_state='possibleBytes',terminal_at=5 WHERE id='handoff-turn'", []).unwrap();
    transaction
        .execute(
            "UPDATE agent_sessions SET state='failed',residency='cold',terminal_at=5 WHERE id=?1",
            [prompt.session_id.as_str()],
        )
        .unwrap();
    transaction.commit().unwrap();
    let next = command(&connection, "explicit-retry", "b");
    assert!(matches!(
        replace_session(&mut connection, next.clone()),
        Err(StoreError::Conflict {
            resource: "handoffDeliveryUnknown"
        })
    ));
    let mut confirmed = next;
    confirmed.confirm_unknown_delivery = true;
    let new = replace_session(&mut connection, confirmed).unwrap();
    assert_ne!(new.session_id, prompt.session_id);
    assert!(
        !super::context_handoff::latest_delivery_ambiguous(&connection, "project", "chat").unwrap()
    );
}

fn frozen_handoff() -> (rusqlite::Connection, PrepareTurnCommand) {
    let mut connection = setup();
    let first = command(&connection, "first", "a");
    let old = replace_session(&mut connection, first).unwrap();
    historical_turn(&connection, &old.session_id);
    let next = command(&connection, "next", "b");
    let target = replace_session(&mut connection, next).unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET state='starting' WHERE id=?1",
            [target.session_id.as_str()],
        )
        .unwrap();
    connection.execute("UPDATE agent_sessions SET state='ready',external_session_id='native-target',capabilities_json=?2 WHERE id=?1", rusqlite::params![target.session_id.as_str(), super::extended_tests::capabilities()]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at) VALUES('project','chat',?1,1,'running',3)", [target.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,created_at,updated_at) VALUES('handoff-turn','project','chat',?1,1,0,'handoff-intent','queued','notWritten',3,3)", [target.session_id.as_str()]).unwrap();
    let mut prompt = PrepareTurnCommand {
        project_id: SageProjectId::parse("project").unwrap(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: SageChatId::parse("chat").unwrap(),
        session_id: target.session_id,
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        expected_projection_revision: target.projection_revision,
        client_intent_id: ClientIntentId::parse("handoff-intent").unwrap(),
        prompt: crate::acp::intent::Prompt::new("prompt", "current request").unwrap(),
        now_millis: 3,
    };
    super::context_handoff::freeze_prompt(&mut connection, &mut prompt).unwrap();
    let transaction = connection.transaction().unwrap();
    super::context_handoff::bind_turn(
        &transaction,
        &prompt,
        "handoff-turn",
        target.projection_revision,
    )
    .unwrap();
    transaction.commit().unwrap();
    (connection, prompt)
}

fn record_handoff_delivery(
    connection: &mut rusqlite::Connection,
    prompt: &PrepareTurnCommand,
    delivery: DeliveryState,
) {
    let transaction = connection.transaction().unwrap();
    crate::acp::context_handoff::delivery::observe(
        &transaction,
        &TurnMutationCommand {
            project_id: prompt.project_id.clone(),
            chat_id: prompt.chat_id.clone(),
            session_id: prompt.session_id.clone(),
            runtime_epoch: prompt.runtime_epoch,
            turn_id: SageTurnId::parse("handoff-turn").unwrap(),
            expected_revision: EntityRevision::ZERO,
            now_millis: 4,
        },
        TurnState::Sending,
        delivery,
        prompt.expected_projection_revision,
    )
    .unwrap();
    transaction.commit().unwrap();
}
