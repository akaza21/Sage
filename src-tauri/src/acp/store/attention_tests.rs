use super::{extended_tests::*, *};
use crate::chat_attention::{contract::Position, read_state, store};
use rusqlite::params;

fn live() -> (
    rusqlite::Connection,
    SageAgentSessionId,
    TurnMutationCommand,
) {
    let mut connection = setup();
    let session = start_session(&mut connection, start(0, "attention-start")).unwrap();
    connection
        .execute("UPDATE agent_sessions SET state='active'", [])
        .unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'running','digest',1,NULL,NULL)", [session.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-live','project-one','chat-one',?1,1,0,'attention-turn','running','correlated',0,3,3,NULL)", [session.session_id.as_str()]).unwrap();
    let command = TurnMutationCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: session.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        turn_id: SageTurnId::parse("turn-live").unwrap(),
        expected_revision: EntityRevision::ZERO,
        now_millis: 4,
    };
    {
        let transaction = connection.transaction().unwrap();
        store::refresh(&transaction, "project-one", "chat-one").unwrap();
        transaction.commit().unwrap();
    }
    (connection, session.session_id, command)
}

fn summary(connection: &rusqlite::Connection) -> crate::chat_attention::contract::ChatAttentionV1 {
    store::get(connection, "project-one", &["chat-one".into()])
        .unwrap()
        .remove(0)
}

fn complete(connection: &mut rusqlite::Connection, command: TurnMutationCommand) {
    apply_turn_terminal(
        connection,
        command,
        crate::acp::lifecycle::TurnState::Completed,
        crate::acp::lifecycle::DeliveryState::Correlated,
    )
    .unwrap();
}

#[test]
fn attention_unmounted_cross_project_requests_and_terminal() {
    let (mut connection, session, command) = live();
    let intent = permission(&session);
    receive_permission_request(&mut connection, &intent, 4).unwrap();
    assert_eq!(summary(&connection).pending_approvals, 1);
    assert!(store::get(&connection, "foreign-project", &["chat-one".into()]).is_err());
    apply_request_state(
        &mut connection,
        RequestStateCommand {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: session,
            turn_id: command.turn_id.clone(),
            runtime_epoch: command.runtime_epoch,
            request_id: intent.request_id,
            state: crate::acp::lifecycle::ServerRequestState::Cancelled,
            now_millis: 5,
        },
    )
    .unwrap();
    assert_eq!(summary(&connection).pending_approvals, 0);
    complete(&mut connection, command);
    let completed = summary(&connection);
    assert!(!completed.running);
    assert_eq!(
        completed.terminal_state,
        Some(crate::acp::lifecycle::TurnState::Completed)
    );
    assert!(completed.terminal_position.is_some());

    connection
        .execute_batch(
            "INSERT INTO chats VALUES('chat-two','project-two',0,'Other project',0,0,0);
        INSERT INTO project_roots VALUES('root-two','project-two','C:/other','c:/other',0);
        INSERT INTO acp_project_usage VALUES('project-two',1,0,0,0,0,0,0);
        UPDATE acp_store_usage SET chat_count=2;",
        )
        .unwrap();
    let mut foreign_start = start(0, "foreign-start");
    foreign_start.project_id = SageProjectId::parse("project-two").unwrap();
    foreign_start.chat_id = SageChatId::parse("chat-two").unwrap();
    let foreign = start_session(&mut connection, foreign_start).unwrap();
    connection
        .execute(
            "UPDATE agent_sessions SET state='active' WHERE id=?1",
            [foreign.session_id.as_str()],
        )
        .unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-two','chat-two',?1,1,'running','digest',1,NULL,NULL)", [foreign.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('foreign-turn','project-two','chat-two',?1,1,0,'foreign-intent','running','correlated',0,3,3,NULL)", [foreign.session_id.as_str()]).unwrap();
    let mut intent = permission(&foreign.session_id);
    intent.project_id = SageProjectId::parse("project-two").unwrap();
    intent.chat_id = SageChatId::parse("chat-two").unwrap();
    intent.turn_id = SageTurnId::parse("foreign-turn").unwrap();
    intent.request_id = SageServerRequestId::parse("foreign-request").unwrap();
    receive_permission_request(&mut connection, &intent, 6).unwrap();
    let foreign_summary = store::get(&connection, "project-two", &["chat-two".into()])
        .unwrap()
        .remove(0);
    assert_eq!(foreign_summary.pending_approvals, 1);
    apply_request_state(
        &mut connection,
        RequestStateCommand {
            project_id: intent.project_id.clone(),
            chat_id: intent.chat_id.clone(),
            session_id: foreign.session_id.clone(),
            turn_id: intent.turn_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            request_id: intent.request_id,
            state: crate::acp::lifecycle::ServerRequestState::Cancelled,
            now_millis: 7,
        },
    )
    .unwrap();
    apply_turn_terminal(
        &mut connection,
        TurnMutationCommand {
            project_id: intent.project_id,
            chat_id: intent.chat_id,
            session_id: foreign.session_id,
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            turn_id: intent.turn_id,
            expected_revision: EntityRevision::ZERO,
            now_millis: 7,
        },
        crate::acp::lifecycle::TurnState::Failed,
        crate::acp::lifecycle::DeliveryState::Correlated,
    )
    .unwrap();
    let failed = store::get(&connection, "project-two", &["chat-two".into()])
        .unwrap()
        .remove(0);
    assert!(!failed.running);
    assert_eq!(failed.pending_approvals, 0);
    assert_eq!(
        failed.terminal_state,
        Some(crate::acp::lifecycle::TurnState::Failed)
    );
    assert_eq!(summary(&connection), completed);
}

#[test]
fn attention_streaming_is_quiet_duplicate_terminal_is_idempotent() {
    let (mut connection, _, command) = live();
    let before = summary(&connection);
    for text in ["first token", "first token and next token"] {
        let payload = crate::acp::intent::NormalizedPayload::Text {
            text: crate::acp::contract::SafeText::new("text", text).unwrap(),
        };
        let digest = super::facts::hex_digest(serde_json::to_string(&payload).unwrap().as_bytes());
        apply_item_ingress(
            &mut connection,
            ItemIngressCommand {
                project_id: command.project_id.clone(),
                chat_id: command.chat_id.clone(),
                session_id: command.session_id.clone(),
                turn_id: command.turn_id.clone(),
                runtime_epoch: command.runtime_epoch,
                external_item_id: crate::acp::intent::ExternalId::new("item", "stream").unwrap(),
                activity_id: None,
                payload,
                payload_digest: digest,
                lifecycle: crate::acp::lifecycle::ItemLifecycle::Streaming,
                tool_state: None,
                now_millis: 5,
            },
        )
        .unwrap();
        assert_eq!(summary(&connection), before);
    }
    complete(&mut connection, command.clone());
    let terminal = summary(&connection);
    assert!(terminal.activity_position > before.activity_position);
    complete(&mut connection, command);
    assert_eq!(summary(&connection), terminal);
}

#[test]
fn attention_repeated_completion_survives_restart() {
    let (mut connection, _, command) = live();
    complete(&mut connection, command.clone());
    let first = summary(&connection);
    read_state::ack(
        &mut connection,
        "project-one",
        "chat-one",
        first.activity_position,
    )
    .unwrap();
    connection.execute("INSERT INTO turns SELECT 'turn-next',project_id,chat_id,session_id,runtime_epoch,1,'attention-next','running','correlated',0,5,5,NULL FROM turns WHERE id='turn-live'", []).unwrap();
    complete(
        &mut connection,
        TurnMutationCommand {
            turn_id: SageTurnId::parse("turn-next").unwrap(),
            ..command
        },
    );
    let second = summary(&connection);
    assert!(second.terminal_position > first.terminal_position);
    let root = std::env::temp_dir().join(format!("sage-attention-{}.db", uuid::Uuid::new_v4()));
    let mut saved = rusqlite::Connection::open(&root).unwrap();
    rusqlite::backup::Backup::new(&connection, &mut saved)
        .unwrap()
        .run_to_completion(256, std::time::Duration::ZERO, None)
        .unwrap();
    drop(saved);
    let reopened = rusqlite::Connection::open(&root).unwrap();
    assert_eq!(summary(&reopened), second);
    drop(reopened);
    std::fs::remove_file(root).unwrap();
}

#[test]
fn attention_read_ack_cannot_consume_newer_completion() {
    let (mut connection, session, command) = live();
    receive_permission_request(&mut connection, &permission(&session), 4).unwrap();
    let observed = summary(&connection).activity_position;
    complete(&mut connection, command);
    let latest = summary(&connection);
    let acknowledged =
        read_state::ack(&mut connection, "project-one", "chat-one", observed).unwrap();
    assert_eq!(acknowledged.read_position, Some(observed));
    assert!(acknowledged.terminal_position > acknowledged.read_position);
    assert!(
        read_state::ack(
            &mut connection,
            "project-one",
            "chat-one",
            latest.activity_position.next().unwrap()
        )
        .is_err()
    );
    assert_eq!(summary(&connection), acknowledged);
    let stale =
        read_state::ack(&mut connection, "project-one", "chat-one", Position::ZERO).unwrap();
    assert_eq!(stale, acknowledged);
}

#[test]
fn attention_mark_unread_never_rewinds_read() {
    let (mut connection, _, command) = live();
    complete(&mut connection, command);
    let before = summary(&connection);
    let read = read_state::ack(
        &mut connection,
        "project-one",
        "chat-one",
        before.activity_position,
    )
    .unwrap();
    let unread =
        read_state::mark_unread(&mut connection, "project-one", "chat-one", read.revision).unwrap();
    assert_eq!(unread.read_position, read.read_position);
    assert_eq!(
        unread.forced_unread_position,
        Some(read.activity_position.next().unwrap())
    );
    assert!(
        read_state::mark_unread(&mut connection, "project-one", "chat-one", read.revision).is_err()
    );
    assert_eq!(summary(&connection), unread);
    assert!(serde_json::from_str::<Position>("\"9223372036854775808\"").is_err());
    assert!(serde_json::from_str::<Position>("\"-1\"").is_err());
}

#[test]
fn attention_side_child_rollup_excludes_native_forks() {
    let (mut connection, session, _) = live();
    connection.execute("INSERT INTO chats VALUES('side-00000000-0000-4000-8000-000000000001','project-one',1,'Side',0,1,1)", []).unwrap();
    connection
        .execute(
            "INSERT INTO chats VALUES('fork-child','project-one',2,'Fork',0,1,1)",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO workbench_tabs(project_id,id,kind,metadata_json) VALUES('project-one','side-00000000-0000-4000-8000-000000000001','chat','{\"parentChatId\":\"chat-one\"}')", []).unwrap();
    connection.execute("INSERT INTO chat_lineage(child_project_id,child_chat_id,parent_project_id,parent_chat_id,relation) VALUES('project-one','fork-child','project-one','chat-one','nativeFork')", []).unwrap();
    for (chat, label) in [
        ("side-00000000-0000-4000-8000-000000000001", "side"),
        ("fork-child", "fork"),
    ] {
        let child = start_session(
            &mut connection,
            StartSessionCommand {
                chat_id: SageChatId::parse(chat).unwrap(),
                ..start(0, label)
            },
        )
        .unwrap();
        connection
            .execute(
                "UPDATE agent_sessions SET state='active' WHERE id=?1",
                [child.session_id.as_str()],
            )
            .unwrap();
        connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one',?1,?2,1,'running','digest',1,NULL,NULL)",params![chat,child.session_id.as_str()]).unwrap();
        let turn = format!("turn-{label}");
        connection.execute("INSERT INTO turns VALUES(?1,'project-one',?2,?3,1,0,?1,'running','correlated',0,3,3,NULL)",params![turn,chat,child.session_id.as_str()]).unwrap();
        let mut intent = permission(&child.session_id);
        intent.chat_id = SageChatId::parse(chat).unwrap();
        intent.turn_id = SageTurnId::parse(turn).unwrap();
        intent.request_id = SageServerRequestId::parse(format!("request-{label}")).unwrap();
        receive_permission_request(&mut connection, &intent, 4).unwrap();
    }
    receive_permission_request(&mut connection, &permission(&session), 4).unwrap();
    assert_eq!(summary(&connection).pending_approvals, 2);
    let children = store::get(
        &connection,
        "project-one",
        &[
            "side-00000000-0000-4000-8000-000000000001".into(),
            "fork-child".into(),
        ],
    )
    .unwrap();
    assert_eq!(
        children[0].lineage.as_ref().unwrap().relation,
        crate::chat_attention::contract::AttentionRelation::SideChat
    );
    assert_eq!(children[1].pending_approvals, 1);
}

#[test]
fn attention_rollback_recovery_overflow_and_v19_upgrade() {
    let (mut connection, _, command) = live();
    let before = summary(&connection);
    {
        let transaction = connection.transaction().unwrap();
        transaction
            .execute(
                "UPDATE turns SET state='failed',terminal_at=9 WHERE id='turn-live'",
                [],
            )
            .unwrap();
        store::refresh(&transaction, "project-one", "chat-one").unwrap();
    }
    assert_eq!(summary(&connection), before);
    crate::acp::recovery::recover_runtime_after_zero_process(
        &mut connection,
        RecoverRuntimeCommand {
            project_id: command.project_id,
            chat_id: command.chat_id,
            session_id: command.session_id,
            runtime_epoch: command.runtime_epoch,
            now_millis: 10,
        },
    )
    .unwrap();
    assert!(!summary(&connection).running);
    assert_eq!(
        summary(&connection).terminal_state,
        Some(crate::acp::lifecycle::TurnState::Failed)
    );
    crate::chat_attention::publisher::assert_overflow_resync_for_test();
    crate::acp::migration::assert_attention_v19_upgrade_for_test();
}

#[test]
fn attention_failed_start_records_a_terminal_without_a_turn() {
    let mut connection = setup();
    let session = start_session(&mut connection, start(0, "failed-start")).unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "UPDATE agent_sessions SET state='failed' WHERE id=?1",
            [session.session_id.as_str()],
        )
        .unwrap();
    store::refresh(&transaction, "project-one", "chat-one").unwrap();
    transaction.commit().unwrap();
    let failed = summary(&connection);
    assert_eq!(
        failed.terminal_state,
        Some(crate::acp::lifecycle::TurnState::Failed)
    );
    assert!(failed.terminal_position.is_some());
    assert!(!failed.running);
}
