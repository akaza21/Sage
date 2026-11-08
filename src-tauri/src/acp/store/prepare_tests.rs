use super::super::{MutationDisposition, prepare_turn};
use super::{command, setup};

#[test]
fn prepare_commits_turn_user_item_and_send_intent_atomically() {
    let mut connection = setup();
    let prepared = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(prepared.disposition, MutationDisposition::Applied);
    for table in ["turns", "items", "session_facts"] {
        let count: i64 = connection
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(count, 1);
    }
    let duplicate = prepare_turn(&mut connection, command("hello")).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    assert_eq!(duplicate.turn_id, prepared.turn_id);
}

#[test]
fn a_session_action_never_titles_its_chat() {
    let title = |connection: &rusqlite::Connection| -> (String, i64) {
        connection
            .query_row(
                "SELECT title,title_is_default FROM chats WHERE id='chat-one'",
                [],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap()
    };
    let untitled = || {
        let connection = setup();
        connection
            .execute_batch(
                "UPDATE chats SET title_is_default=1;
                 CREATE TABLE workbench_tabs(project_id TEXT,id TEXT,kind TEXT,title TEXT,updated_at INTEGER);",
            )
            .unwrap();
        connection
    };
    let mut connection = untitled();
    let before = title(&connection);
    let mut action = command("/compact");
    action.prompt.action =
        Some(serde_json::from_value(serde_json::json!({"kind": "compact"})).unwrap());
    prepare_turn(&mut connection, action).unwrap();
    assert_eq!(title(&connection), before);

    let mut connection = untitled();
    prepare_turn(&mut connection, command("Fix the sidebar")).unwrap();
    assert_eq!(title(&connection).0, "Chat: Fix the sidebar");
}
