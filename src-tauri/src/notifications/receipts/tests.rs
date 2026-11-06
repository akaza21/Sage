use super::*;
use crate::acp::contract::{ProjectionRevision, SageAgentSessionId, SageChatId, SageProjectId};

fn fixture() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;
        CREATE TABLE chats(id TEXT PRIMARY KEY,project_id TEXT);
        CREATE TABLE turns(id TEXT PRIMARY KEY,chat_id TEXT REFERENCES chats(id) ON DELETE CASCADE,project_id TEXT,session_id TEXT,state TEXT);
        CREATE TABLE server_requests(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,state TEXT,kind TEXT);
        INSERT INTO chats VALUES ('chat','project');").unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/021_turn_notifications.sql"
        ))
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns VALUES ('turn','chat','project','session','running')",
            [],
        )
        .unwrap();
    connection
}
fn notice() -> ProjectionChangedV1 {
    ProjectionChangedV1 {
        project_id: SageProjectId::parse("project").unwrap(),
        chat_id: SageChatId::parse("chat").unwrap(),
        session_id: SageAgentSessionId::parse("session").unwrap(),
        projection_revision: ProjectionRevision::new(1).unwrap(),
        child_projection_epoch: 0,
    }
}
fn transition(connection: &Connection, state: &str) {
    connection
        .execute("UPDATE turns SET state=?1 WHERE id='turn'", [state])
        .unwrap();
}

#[test]
fn exact_committed_events_survive_later_turns_and_conflicts_never_replay() {
    let connection = fixture();
    transition(&connection, "waitingApproval");
    transition(&connection, "running");
    transition(&connection, "waitingApproval");
    transition(&connection, "completed");
    connection
        .execute(
            "INSERT INTO turns VALUES ('next','chat','project','session','running')",
            [],
        )
        .unwrap();
    let events = claim(&connection, &notice()).unwrap();
    assert_eq!(
        events.iter().map(|(_, event)| *event).collect::<Vec<_>>(),
        vec![Event::Completed]
    );
    assert_eq!(
        target(&connection, events[0].0).unwrap(),
        Some(OpenChat {
            project_id: "project".into(),
            chat_id: "chat".into()
        })
    );
    // The OS may have delivered before returning an error. Never retry it.
    assert!(claim(&connection, &notice()).unwrap().is_empty());
}

#[test]
fn failure_is_captured_but_rollback_and_cancellation_do_not_notify() {
    let connection = fixture();
    connection
        .execute_batch("BEGIN; UPDATE turns SET state='failed'; ROLLBACK;")
        .unwrap();
    assert!(claim(&connection, &notice()).unwrap().is_empty());
    transition(&connection, "cancelled");
    assert!(claim(&connection, &notice()).unwrap().is_empty());
    transition(&connection, "deliveryUnknown");
    transition(&connection, "failed");
    assert_eq!(
        claim(&connection, &notice())
            .unwrap()
            .iter()
            .map(|(_, event)| *event)
            .collect::<Vec<_>>(),
        vec![Event::Failed]
    );
}

#[test]
fn deleted_chat_loses_receipts_and_old_clicks_cannot_target_reused_rowids() {
    let connection = fixture();
    transition(&connection, "completed");
    let old = claim(&connection, &notice()).unwrap()[0].0;
    connection.execute("DELETE FROM chats", []).unwrap();
    assert_eq!(target(&connection, old).unwrap(), None);
    connection.execute_batch("INSERT INTO chats VALUES ('chat','project'); INSERT INTO turns VALUES ('turn','chat','project','session','completed');").unwrap();
    assert!(claim(&connection, &notice()).unwrap()[0].0 > old);
    assert_eq!(target(&connection, old).unwrap(), None);
}

#[test]
fn receipt_batches_are_bounded_and_claims_survive_reopening() {
    let directory =
        std::env::temp_dir().join(format!("sage-notice-receipts-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("receipts.db");
    let mut disk = Connection::open(&path).unwrap();
    let connection = fixture();
    rusqlite::backup::Backup::new(&connection, &mut disk)
        .unwrap()
        .run_to_completion(32, std::time::Duration::ZERO, None)
        .unwrap();
    for ordinal in 0..10 {
        disk.execute(
            "INSERT INTO turns VALUES (?1,'chat','project','session','completed')",
            [format!("turn-{ordinal}")],
        )
        .unwrap();
    }
    assert_eq!(claim(&disk, &notice()).unwrap().len(), 4);
    drop(disk);
    let reopened = Connection::open(&path).unwrap();
    assert_eq!(claim(&reopened, &notice()).unwrap().len(), 4);
    assert_eq!(claim(&reopened, &notice()).unwrap().len(), 2);
    assert!(claim(&reopened, &notice()).unwrap().is_empty());
    drop(reopened);
    std::fs::remove_dir_all(directory).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn bounded_receipt_resource_probe() {
    fn rss_kib() -> u64 {
        std::fs::read_to_string("/proc/self/status")
            .unwrap()
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .and_then(|value| value.split_whitespace().next())
                    .and_then(|value| value.parse().ok())
            })
            .unwrap()
    }
    let baseline = rss_kib();
    let connection = fixture();
    transition(&connection, "completed");
    let started = std::time::Instant::now();
    let mut peak = rss_kib();
    for index in 0..10_000 {
        let claims = claim(&connection, &notice()).unwrap();
        assert!(claims.len() <= 1);
        if index % 100 == 0 {
            peak = peak.max(rss_kib());
        }
    }
    let micros = started.elapsed().as_micros();
    assert_eq!(
        connection
            .query_row(
                "SELECT COUNT(*) FROM turn_notification_receipts",
                [],
                |row| row.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    drop(connection);
    eprintln!(
        "notification receipt probe: 10000 publications; baseline_kib={baseline}; peak_kib={peak}; settled_kib={}; elapsed_us={micros}",
        rss_kib()
    );
}

#[test]
fn pending_permission_notifies_a_running_turn_but_automatic_denial_does_not() {
    let connection = fixture();
    connection.execute_batch("INSERT INTO server_requests VALUES ('request','project','chat','session','turn','received','permission');").unwrap();
    assert_eq!(claim(&connection, &notice()).unwrap()[0].1, Event::Approval);
    assert!(claim(&connection, &notice()).unwrap().is_empty());
    let denied = fixture();
    denied.execute_batch("BEGIN; INSERT INTO server_requests VALUES ('request','project','chat','session','turn','received','permission'); UPDATE server_requests SET state='resolvedDeclined'; COMMIT;").unwrap();
    assert!(claim(&denied, &notice()).unwrap().is_empty());
}
