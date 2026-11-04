use std::{fs, path::Path};

use crate::{
    acp::{
        contract::{SageChatId, SageProjectId},
        store::load_chat_page,
    },
    persistence::Database,
};

fn page(database: &Database, project: &str, chat: &str) -> crate::acp::projection::AcpChatPage {
    load_chat_page(
        &database.connection(),
        SageProjectId::parse(project).unwrap(),
        SageChatId::parse(chat).unwrap(),
        None,
    )
    .unwrap()
}

#[test]
fn new_project_reopens_empty_and_first_prompt_installs_only_real_turn_usage() {
    let base = std::env::temp_dir().join(format!("sage-cp28-startup-{}", uuid::Uuid::new_v4()));
    let root = base.join("project");
    fs::create_dir_all(&root).unwrap();
    let path = base.join("sage.db");
    let database = Database::open(&path).unwrap();
    let project = database.create_project_from_root(&root).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let chat = snapshot.active_tab_id.unwrap();
    assert!(snapshot.messages.is_empty());
    assert!(snapshot.session_id.is_none());
    assert_eq!(snapshot.tabs[0].title, "Chat: Chat");
    assert!(!snapshot.tabs[0].pinned);
    let initial = page(&database, &project.id, &chat);
    assert!(initial.turns.as_slice().is_empty());
    assert!(initial.sessions.as_slice().is_empty());
    assert_eq!(usage(&database), (1, 0, 0, 0, 0, 0));
    drop(database);

    let reopened = Database::open(&path).unwrap();
    assert!(reopened.bootstrap_snapshot().unwrap().messages.is_empty());
    assert!(
        page(&reopened, &project.id, &chat)
            .turns
            .as_slice()
            .is_empty()
    );
    let result = reopened
        .append_simulator_turn(&chat, "first prompt", "real answer")
        .unwrap();
    assert_eq!(result.messages.len(), 2);
    assert_eq!(result.messages[0].text, "first prompt");
    assert_eq!(result.messages[1].text, "real answer");
    let history = page(&reopened, &project.id, &chat);
    assert_eq!(history.title.as_str(), "Chat: first prompt");
    assert_eq!(history.turns.as_slice().len(), 1);
    assert_eq!(history.turns.as_slice()[0].items.as_slice().len(), 2);
    let counters = usage(&reopened);
    assert_eq!(
        (counters.0, counters.1, counters.2, counters.3, counters.4),
        (1, 1, 1, 2, 1)
    );
    assert!(counters.5 > 0);
    drop(reopened);

    let reopened = Database::open(&path).unwrap();
    assert_eq!(reopened.bootstrap_snapshot().unwrap().messages.len(), 2);
    assert_eq!(
        page(&reopened, &project.id, &chat).turns.as_slice().len(),
        1
    );
    assert_eq!(usage(&reopened), counters);
    drop(reopened);
    fs::remove_dir_all(base).unwrap();
}

#[test]
fn fresh_local_project_first_chat_loads_empty_history() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    let project = database.ensure_local_project().unwrap();
    assert!(project.roots.is_empty());
    let snapshot = database.bootstrap_snapshot().unwrap();
    let chat = snapshot.active_tab_id.unwrap();

    let history = page(&database, &project.id, &chat);
    assert!(history.turns.as_slice().is_empty());
    assert!(history.sessions.as_slice().is_empty());
    assert!(history.current_session_id.is_none());
    assert!(history.next_cursor.is_none());

    database
        .append_simulator_turn(&chat, "first prompt", "real answer")
        .unwrap();
    let history = page(&database, &project.id, &chat);
    assert_eq!(history.turns.as_slice().len(), 1);
    assert_eq!(history.turns.as_slice()[0].items.as_slice().len(), 2);
}

#[test]
fn local_project_starts_without_a_simulator_session_or_turn() {
    let database = Database::open(Path::new(":memory:")).unwrap();
    database.ensure_local_project().unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    assert_eq!(snapshot.project.unwrap().name, "Local");
    assert!(snapshot.active_tab_id.is_some());
    assert!(snapshot.messages.is_empty());
    assert!(snapshot.session_id.is_none());
    let rows: (i64, i64) = database
        .connection()
        .query_row(
            "SELECT (SELECT COUNT(*) FROM turns),(SELECT COUNT(*) FROM agent_sessions)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(rows, (0, 0));
    assert_eq!(usage(&database), (1, 0, 0, 0, 0, 0));
}

fn usage(database: &Database) -> (i64, i64, i64, i64, i64, i64) {
    database
        .connection()
        .query_row(
            "SELECT chat_count,session_count,turn_count,item_count,fact_count,shared_payload_bytes
         FROM acp_store_usage WHERE singleton=1",
            [],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .unwrap()
}
