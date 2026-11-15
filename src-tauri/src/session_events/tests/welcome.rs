use super::*;
use crate::acp::store::load_chat_page;
use sage_acp_protocol::contract::{SageChatId, SageProjectId};

#[test]
fn hidden_welcome_does_not_skip_real_turns_or_create_a_trailing_page() {
    for count in [20, 21] {
        let database = Database::open(Path::new(":memory:")).unwrap();
        let session = legacy_welcome_session(&database);
        let chat = chat(&database, &session);
        for ordinal in 0..count {
            database
                .append_simulator_turn(&chat, &format!("prompt-{ordinal}"), "answer")
                .unwrap();
        }
        let before = durable_chat_state(&database, &chat);
        let first = history(&database, &chat);
        assert_eq!(first.turns.as_slice().len(), 20);
        let mut ids = first
            .turns
            .as_slice()
            .iter()
            .map(|turn| turn.id.as_str().to_owned())
            .collect::<Vec<_>>();
        if count == 20 {
            assert!(first.next_cursor.is_none());
        } else {
            let second = load_chat_page(
                &database.connection(),
                first.project_id,
                first.chat_id,
                Some(first.next_cursor.unwrap()),
            )
            .unwrap();
            assert_eq!(second.turns.as_slice().len(), 1);
            assert!(second.next_cursor.is_none());
            ids.extend(
                second
                    .turns
                    .as_slice()
                    .iter()
                    .map(|turn| turn.id.as_str().to_owned()),
            );
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), count as usize);
        assert_eq!(durable_chat_state(&database, &chat), before);
        assert_ledgers_exact(&database.connection());
    }
}

fn history(database: &Database, chat: &str) -> crate::acp::projection::AcpChatPage {
    let project: String = database
        .connection()
        .query_row("SELECT project_id FROM chats WHERE id=?1", [chat], |row| {
            row.get(0)
        })
        .unwrap();
    load_chat_page(
        &database.connection(),
        SageProjectId::parse(project).unwrap(),
        SageChatId::parse(chat).unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn hidden_welcome_preserves_same_turn_item_continuation_without_duplicates() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let seed = legacy_welcome_session(&database);
    let chat = chat(&database, &seed);
    let real = database
        .append_simulator_turn(&chat, "prompt", "answer")
        .unwrap();
    {
        let mut connection = database.write_connection().unwrap();
        let transaction = connection.transaction().unwrap();
        let (template, bytes): (String, i64) = transaction
            .query_row(
                "SELECT id,payload_bytes FROM items WHERE session_id=?1 AND ordinal=1",
                [&real.session_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        for ordinal in 2..257 {
            transaction.execute(
                "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
                 origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,created_at,
                 updated_at,terminal_at,activity_id,tool_state,external_item_id)
                 SELECT ?1,project_id,chat_id,session_id,turn_id,runtime_epoch,?2,
                 origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,revision,created_at,
                 updated_at,terminal_at,activity_id,tool_state,external_item_id FROM items WHERE id=?3",
                params![format!("history-item-{ordinal}"), ordinal, template],
            ).unwrap();
        }
        let extra_bytes = bytes * 255;
        transaction
            .execute(
                "UPDATE acp_session_usage SET item_count=item_count+255,item_bytes=item_bytes+?2,
             payload_bytes=payload_bytes+?2 WHERE session_id=?1",
                params![real.session_id, extra_bytes],
            )
            .unwrap();
        transaction.execute(
            "UPDATE acp_project_usage SET item_count=item_count+255,payload_bytes=payload_bytes+?2
             WHERE project_id=(SELECT project_id FROM chats WHERE id=?1)", params![chat, extra_bytes],
        ).unwrap();
        transaction
            .execute(
                "UPDATE acp_store_usage SET item_count=item_count+255,item_bytes=item_bytes+?1,
             shared_payload_bytes=shared_payload_bytes+?1 WHERE singleton=1",
                [extra_bytes],
            )
            .unwrap();
        transaction.commit().unwrap();
    }
    let before = durable_chat_state(&database, &chat);
    let first = history(&database, &chat);
    assert_eq!(first.turns.as_slice().len(), 1);
    assert_eq!(first.turns.as_slice()[0].items.as_slice().len(), 256);
    let second = load_chat_page(
        &database.connection(),
        first.project_id.clone(),
        first.chat_id.clone(),
        Some(first.next_cursor.clone().unwrap()),
    )
    .unwrap();
    assert_eq!(second.turns.as_slice().len(), 1);
    assert_eq!(second.turns.as_slice()[0].id, first.turns.as_slice()[0].id);
    assert_eq!(second.turns.as_slice()[0].items.as_slice().len(), 1);
    assert!(second.next_cursor.is_none());
    let mut ids = first.turns.as_slice()[0]
        .items
        .as_slice()
        .iter()
        .chain(second.turns.as_slice()[0].items.as_slice())
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    ids.sort();
    ids.dedup();
    assert_eq!(ids.len(), 257);
    assert_eq!(durable_chat_state(&database, &chat), before);
    assert_ledgers_exact(&database.connection());
}

#[test]
fn historical_seed_provenance_is_omitted_without_deleting_rows_or_usage() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session = legacy_welcome_session(&database);
    let chat = chat(&database, &session);
    let before = durable_chat_state(&database, &chat);
    assert!(database.messages(&session).unwrap().is_empty());
    assert!(history(&database, &chat).turns.as_slice().is_empty());
    assert_eq!(durable_chat_state(&database, &chat), before);
    assert_ledgers_exact(&database.connection());
    let result = database
        .append_simulator_turn(
            &chat,
            "real prompt",
            "This project is ready. Choose a harness and begin a persistent chat.",
        )
        .unwrap();
    assert_eq!(result.messages.len(), 2);
    assert_eq!(
        result.messages[1].text,
        "This project is ready. Choose a harness and begin a persistent chat."
    );
    assert_eq!(history(&database, &chat).turns.as_slice().len(), 1);
    assert_ledgers_exact(&database.connection());
}

#[test]
fn an_assistant_only_zero_duration_turn_without_seed_fact_provenance_is_preserved() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let session = historical_simulator_session(&database, "observedTurn");
    let chat = chat(&database, &session);
    assert_eq!(database.messages(&session).unwrap().len(), 1);
    let retained = history(&database, &chat);
    assert_eq!(retained.turns.as_slice().len(), 1);
    assert_eq!(
        retained.turns.as_slice()[0].created_at,
        retained.turns.as_slice()[0].terminal_at.unwrap()
    );
}
