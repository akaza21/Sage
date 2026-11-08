use super::*;
use crate::turn_workspace::{capture, freeze_session_for_test};

fn connection(path: &Path) -> Connection {
    let connection = Connection::open(path).unwrap();
    connection.execute_batch("CREATE TABLE IF NOT EXISTS agent_sessions(id TEXT PRIMARY KEY,project_id TEXT);
        CREATE TABLE IF NOT EXISTS turns(session_id TEXT,client_intent_id TEXT,terminal_at INTEGER);
        CREATE TABLE IF NOT EXISTS acp_session_usage(session_id TEXT PRIMARY KEY,payload_bytes INTEGER,revision INTEGER);
        CREATE TABLE IF NOT EXISTS acp_project_usage(project_id TEXT PRIMARY KEY,payload_bytes INTEGER,revision INTEGER);
        CREATE TABLE IF NOT EXISTS acp_store_usage(singleton INTEGER PRIMARY KEY,shared_payload_bytes INTEGER,revision INTEGER);
        INSERT OR IGNORE INTO agent_sessions VALUES('a','project'),('b','project');
        INSERT OR IGNORE INTO acp_session_usage VALUES('a',0,0),('b',0,0);
        INSERT OR IGNORE INTO acp_project_usage VALUES('project',0,0);
        INSERT OR IGNORE INTO acp_store_usage VALUES(1,0,0);").unwrap();
    let exists: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='acp_turn_workspace_changes')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    if !exists {
        connection
            .execute_batch(include_str!(
                "../../migrations/019_turn_workspace_changes.sql"
            ))
            .unwrap();
    }
    connection
}

fn root() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("sage-ledger-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(root.join("repo")).unwrap();
    assert!(
        crate::test_git::command()
            .args(["init", "-q"])
            .current_dir(root.join("repo"))
            .status()
            .unwrap()
            .success()
    );
    root.canonicalize().unwrap()
}

fn baseline(root: &Path) -> WorkspaceMark {
    tauri::async_runtime::block_on(capture(root)).unwrap()
}

fn terminal(connection: &mut Connection, session: &str, key: &str) {
    connection.execute("INSERT INTO turns SELECT ?1,?2,NULL WHERE NOT EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND client_intent_id=?2)",params![session,key]).unwrap();
    freeze_session_for_test(connection, session).unwrap();
    let transaction = connection.transaction().unwrap();
    transaction
        .execute(
            "UPDATE turns SET terminal_at=1 WHERE session_id=?1 AND client_intent_id=?2",
            params![session, key],
        )
        .unwrap();
    settle_terminal(&transaction, session).unwrap();
    transaction.commit().unwrap();
}

#[test]
fn terminal_snapshot_is_durable_and_later_chat_edits_never_rewrite_it() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = root();
    let repo = root.join("repo");
    let path = root.join("ledger.sqlite");
    let mut database = connection(&path);
    remember(
        &mut database,
        "a",
        "first",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    std::fs::write(repo.join("a.txt"), "A").unwrap();
    terminal(&mut database, "a", "first");
    std::fs::write(repo.join("b.txt"), "B after A finished").unwrap();
    drop(database);
    let mut reopened = connection(&path);
    let changes = load(&reopened, "a", "first", "root").unwrap();
    assert!(changes.tracked);
    assert_eq!(
        changes
            .changes
            .iter()
            .map(|change| change.relative_path.as_str())
            .collect::<Vec<_>>(),
        ["a.txt"]
    );
    let transaction = reopened.transaction().unwrap();
    settle_terminal(&transaction, "a").unwrap();
    transaction.commit().unwrap();
    assert_eq!(load(&reopened, "a", "first", "root").unwrap(), changes);
    assert!(!load(&reopened, "a", "first", "other-root").unwrap().tracked);
    assert!(!load(&reopened, "b", "first", "root").unwrap().tracked);
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn overlapping_chats_fail_closed_for_both_summaries() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = root();
    let repo = root.join("repo");
    let mut database = connection(Path::new(":memory:"));
    remember(
        &mut database,
        "a",
        "first",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    remember(
        &mut database,
        "b",
        "second",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    std::fs::write(repo.join("shared.txt"), "Cannot attribute").unwrap();
    terminal(&mut database, "a", "first");
    terminal(&mut database, "b", "second");
    assert_eq!(
        load(&database, "a", "first", "root").unwrap(),
        TurnWorkspaceChanges::untracked()
    );
    assert_eq!(
        load(&database, "b", "second", "root").unwrap(),
        TurnWorkspaceChanges::untracked()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn many_later_turns_do_not_evict_an_active_baseline_and_duplicates_do_not_reset_it() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = root();
    let repo = root.join("repo");
    let mut database = connection(Path::new(":memory:"));
    remember(
        &mut database,
        "a",
        "long",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    for ordinal in 0..70 {
        let key = format!("other-{ordinal}");
        remember(
            &mut database,
            "b",
            &key,
            "other-root",
            Path::new("elsewhere"),
            None,
        )
        .unwrap();
        terminal(&mut database, "b", &key);
    }
    std::fs::write(repo.join("long.txt"), "Still tracked").unwrap();
    remember(
        &mut database,
        "a",
        "long",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    terminal(&mut database, "a", "long");
    assert_eq!(
        load(&database, "a", "long", "root").unwrap().changes[0].relative_path,
        "long.txt"
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn restart_invalidates_unfinished_attribution_without_losing_terminal_history() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = root();
    let repo = root.join("repo");
    let path = root.join("ledger.sqlite");
    let mut database = connection(&path);
    remember(
        &mut database,
        "a",
        "unfinished",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    database
        .execute("INSERT INTO turns VALUES('a','unfinished',NULL)", [])
        .unwrap();
    drop(database);
    std::fs::write(repo.join("after-restart.txt"), "External edits").unwrap();
    let mut reopened = connection(&path);
    invalidate_active(&reopened).unwrap();
    reopened
        .execute("UPDATE turns SET terminal_at=2", [])
        .unwrap();
    let transaction = reopened.transaction().unwrap();
    settle_terminal(&transaction, "a").unwrap();
    transaction.commit().unwrap();
    assert_eq!(
        load(&reopened, "a", "unfinished", "root").unwrap(),
        TurnWorkspaceChanges::untracked()
    );
    drop(reopened);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn active_baseline_capacity_fails_closed_without_discarding_existing_marks() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let mut database = connection(Path::new(":memory:"));
    for ordinal in 0..MAX_ACTIVE_BASELINES {
        remember(
            &mut database,
            "a",
            &format!("active-{ordinal}"),
            "root",
            Path::new("repo"),
            None,
        )
        .unwrap();
    }
    assert!(
        remember(
            &mut database,
            "b",
            "overflow",
            "root",
            Path::new("repo"),
            None
        )
        .is_err()
    );
    let count: i64 = database
        .query_row(
            "SELECT COUNT(*) FROM acp_turn_workspace_changes WHERE result_json IS NULL",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(count, MAX_ACTIVE_BASELINES);
    forget_unprepared(&database, "a", "active-0").unwrap();
    remember(
        &mut database,
        "b",
        "replacement",
        "root",
        Path::new("repo"),
        None,
    )
    .unwrap();
}

#[test]
fn registration_preserves_overlap_if_another_chat_finishes_during_baseline_capture() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let root = root();
    let repo = root.join("repo");
    let mut database = connection(Path::new(":memory:"));
    remember(
        &mut database,
        "a",
        "first",
        "root",
        &repo,
        Some(baseline(&repo)),
    )
    .unwrap();
    database
        .execute("INSERT INTO turns VALUES('a','first',NULL)", [])
        .unwrap();
    remember(&mut database, "b", "second", "root", &repo, None).unwrap();
    std::fs::write(repo.join("a.txt"), "A changed while B captured").unwrap();
    terminal(&mut database, "a", "first");
    publish_baseline(&database, "b", "second", Some(baseline(&repo))).unwrap();
    std::fs::write(repo.join("b.txt"), "B").unwrap();
    terminal(&mut database, "b", "second");
    assert!(!load(&database, "a", "first", "root").unwrap().tracked);
    assert!(!load(&database, "b", "second", "root").unwrap().tracked);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn terminal_summary_only_releases_reserved_payload_even_at_the_global_limit() {
    let _capture_guard = crate::turn_workspace::capture_test_lock();
    let mut database = connection(Path::new(":memory:"));
    remember(
        &mut database,
        "a",
        "limited",
        "root",
        Path::new("repo"),
        None,
    )
    .unwrap();
    database
        .execute(
            "UPDATE acp_store_usage SET shared_payload_bytes=?1",
            [LIMITS.durable.shared_payload_bytes_global as i64],
        )
        .unwrap();
    terminal(&mut database, "a", "limited");
    let charged: i64 = database
        .query_row(
            "SELECT shared_payload_bytes FROM acp_store_usage",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert!(charged < LIMITS.durable.shared_payload_bytes_global as i64);
    assert_eq!(
        load(&database, "a", "limited", "root").unwrap(),
        TurnWorkspaceChanges::untracked()
    );
}

#[test]
fn absent_session_does_not_reserve_workspace_payload() {
    let root = root();
    let mut database = connection(&root.join("ledger.sqlite"));
    assert!(!remember(&mut database, "absent", "intent", "root", &root, None).unwrap());
    let usage: i64 = database
        .query_row(
            "SELECT shared_payload_bytes FROM acp_store_usage",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(usage, 0);
    assert_eq!(
        database
            .query_row(
                "SELECT COUNT(*) FROM acp_turn_workspace_changes",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        0
    );
    drop(database);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_baseline_is_incomplete_but_unsupported_workspace_has_no_notice() {
    let mut connection = connection(Path::new(":memory:"));
    remember(
        &mut connection,
        "a",
        "unsupported",
        "root",
        Path::new("repo"),
        None,
    )
    .unwrap();
    terminal(&mut connection, "a", "unsupported");
    assert_eq!(
        load(&connection, "a", "unsupported", "root").unwrap(),
        TurnWorkspaceChanges::untracked()
    );
    remember(
        &mut connection,
        "a",
        "failed",
        "root",
        Path::new("repo"),
        None,
    )
    .unwrap();
    publish_baseline_failure(&connection, "a", "failed").unwrap();
    terminal(&mut connection, "a", "failed");
    assert_eq!(
        load(&connection, "a", "failed", "root").unwrap(),
        TurnWorkspaceChanges::incomplete()
    );
}
