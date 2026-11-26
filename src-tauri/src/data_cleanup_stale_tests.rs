use super::*;
use crate::data_cleanup_stale::{StaleChatStore, stale_scope};

const OLD: &str = "UPDATE chats SET updated_at=1000 WHERE id=?1";

fn running(database: &Database, chat: &str) {
    database
        .connection()
        .execute_batch(&format!(
            "INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,started_at)
               SELECT project_id,chat_id,id,100,'running',1 FROM agent_sessions WHERE chat_id='{chat}';
             INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,
                               state,delivery_state,revision,created_at,updated_at)
               SELECT 'turn-live',project_id,chat_id,id,100,1000,'intent-live','running','correlated',0,1,1
               FROM agent_sessions WHERE chat_id='{chat}';"
        ))
        .unwrap();
}

#[test]
fn stale_cleanup_deletes_only_old_idle_unpinned_chats_in_every_project() {
    let fixture = fixture();
    let Fixture {
        root,
        database,
        project,
    } = &fixture;
    let pinned = database
        .bootstrap_snapshot()
        .unwrap()
        .active_tab_id
        .unwrap();
    database
        .connection()
        .execute("UPDATE workbench_tabs SET pinned=1 WHERE id=?1", [&pinned])
        .unwrap();
    let (old, _) = with_history(database, project);
    let (live, _) = with_history(database, project);
    running(database, &live);
    let fresh = new_chat(database, project);
    std::fs::create_dir_all(root.join("b")).unwrap();
    let other = database.create_project_with_chat(&root.join("b")).unwrap();
    let other_chat: String = database
        .connection()
        .query_row(
            "SELECT id FROM chats WHERE project_id=?1",
            [&other.id],
            |row| row.get(0),
        )
        .unwrap();
    for chat in [&pinned, &old, &live, &other_chat] {
        database.connection().execute(OLD, [chat]).unwrap();
    }
    let now = sage_core::time::now_millis();
    let scope = stale_scope(5000, now).unwrap();

    assert_eq!(database.stale_chat_count(scope).unwrap(), 2);
    let projects = database.stale_projects(scope).unwrap();
    assert_eq!(projects.len(), 2);
    let mut deleted = 0;
    for project in &projects {
        let outcome = database.clean_up(project, scope).unwrap();
        assert!(outcome.refusal.is_none(), "{:?}", outcome.refusal);
        deleted += outcome.deleted;
    }

    assert_eq!(deleted, 2);
    let exists = |chat: &str| count(database, "SELECT COUNT(*) FROM chats WHERE id=?1", chat);
    assert_eq!((exists(&old), exists(&other_chat)), (0, 0));
    assert_eq!((exists(&live), exists(&pinned), exists(&fresh)), (1, 1, 1));
    // The emptied Project stays; only `remove_project` removes a Project.
    assert_eq!(
        count(
            database,
            "SELECT COUNT(*) FROM projects WHERE id=?1",
            &other.id
        ),
        1
    );
    assert_eq!(database.stale_chat_count(scope).unwrap(), 0);
    fixture.finish();
}

#[test]
fn a_stale_cutoff_must_be_at_least_a_day_old() {
    let now = sage_core::time::now_millis();
    let day = 24 * 60 * 60 * 1000;
    assert!(stale_scope(now, now).is_err());
    assert!(stale_scope(now - day + 1, now).is_err());
    assert!(stale_scope(0, now).is_err());
    assert_eq!(
        stale_scope(now - day, now).unwrap(),
        CleanupScope::Stale(now - day)
    );
}
