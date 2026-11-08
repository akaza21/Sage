//! A permission the adapter abandons after its Turn ended is settled durably.

use sha2::{Digest, Sha256};

use super::*;

const EXTERNAL_REQUEST: &str = "permission-7";

/// The Sage id the pump gives a harness permission request in epoch 3.
fn scoped_request_id() -> String {
    let session = Sha256::digest(SAGE_SESSION.as_bytes());
    let external = Sha256::digest(EXTERNAL_REQUEST.as_bytes());
    format!("permission:{session:x}:3:{external:x}")
}

/// A request still open on the ended Turn, as a Turn's terminal leaves it:
/// the permission item is no longer waiting but the request row is `asked`.
fn seed_open_request(database: &Database) {
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO items VALUES('permission-item','project-one','chat-one',?1,'turn-one',3,9,'primary',NULL,'permissionRequest','started','waitingForApproval',?2,'{}',2,'abababababababababababababababababababababababababababababababab',0,0,0,NULL)",
            rusqlite::params![SAGE_SESSION, EXTERNAL_REQUEST],
        )
        .expect("seed item");
    connection
        .execute(
            "INSERT INTO server_requests(id,project_id,chat_id,session_id,turn_id,runtime_epoch,
                binding_digest,external_request_id,kind,payload_digest_version,payload_digest,
                state,normalized_payload_json,revision,created_at,updated_at,resolved_at)
             VALUES(?1,'project-one','chat-one',?2,'turn-one',3,?4,?3,'permission',1,
                'abababababababababababababababababababababababababababababababab',
                'asked','{}',0,0,0,NULL)",
            rusqlite::params![
                scoped_request_id(),
                SAGE_SESSION,
                EXTERNAL_REQUEST,
                "07".repeat(32)
            ],
        )
        .expect("seed request");
    drop(connection);
}

fn request_state(database: &Database) -> String {
    let connection = database.connection();
    let state = connection
        .query_row(
            "SELECT state FROM server_requests WHERE id=?1",
            [scoped_request_id()],
            |row| row.get(0),
        )
        .expect("request row");
    drop(connection);
    state
}

#[test]
fn a_permission_cancelled_after_its_turn_ended_settles_its_request_row() {
    let mut runtime = ClaudeRuntime::fixture();
    ready(&mut runtime);
    // The adapter asked request 7 in turn-one and Sage has not answered it.
    let opened_in = identity(
        runtime.binding_digest,
        runtime.runtime_epoch,
        &runtime.session_id,
        ExternalId::new("externalSessionId", SESSION).expect("session"),
        Some(ExternalId::new("turnId", "claude-turn-one").expect("turn")),
        Some(RequiredText::new("clientMessageId", "turn-one").expect("client message")),
    );
    runtime
        .reserve_denial(
            EXTERNAL_REQUEST,
            opened_in,
            "tool-1".into(),
            json!(7),
            Some("allow".into()),
            None,
        )
        .expect("open request");
    let database = migrated_database(&runtime);
    let intent = session_intent()
        .bind_turn(ActiveTurnIntent::store_minted(turn("one")).expect("turn one intent"));

    tokio::runtime::Runtime::new()
        .expect("tokio runtime")
        .block_on(async {
            let (handle, runner) = JournalHandle::new(DatabaseAcpStore::new(database.clone()));
            let handle = CloseOnDrop(handle);
            let task = runner.spawn();
            let mut pump = AdapterPump::new(
                runtime,
                SqliteAdapterBatchJournal::new(handle.0.clone(), intent.clone()),
                Mapper,
                Arc::new(ProjectionMailbox::default()),
                intent,
            )
            .expect("pump");
            drive_turn(&mut pump, "turn-one", "ok", 10).await;
            let mut parts = pump
                .try_into_completed_parts()
                .map_err(|boxed| boxed.1)
                .expect("the Turn handed the session back");
            seed_open_request(&database);
            assert_eq!(request_state(&database), "asked");

            let mut cancel = serde_json::to_vec(&json!({
                "jsonrpc":"2.0","method":"$/cancel_request","params":{"requestId":7}
            }))
            .expect("frame");
            cancel.push(b'\n');
            bounded(
                "abandoned permission",
                parts.accept_idle_stdout_chunk(&cancel, 20, &mut Sink::default()),
            )
            .await
            .expect("idle cancel accepted");
            assert_eq!(request_state(&database), "cancelled");

            handle.0.close_when_drained();
            bounded("journal drain", task)
                .await
                .expect("journal drained");
        });
}
