use rusqlite::{Error, ffi, params};
use serde_json::Value;

pub(super) fn insert_request(
    connection: &rusqlite::Connection,
    id: &str,
    runtime_epoch: i64,
    binding_digest: &str,
    payload_digest: &str,
    state: &str,
    resolved_at: Option<i64>,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO server_requests(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
           external_request_id,kind,payload_digest_version,payload_digest,state,
           normalized_payload_json,created_at,updated_at,resolved_at
         ) VALUES (?1,'project-a','chat-a','session-a','turn-a',?2,?3,
                   ?1,'permission',1,?4,?5,'{}',1,1,?6)",
        params![
            id,
            runtime_epoch,
            binding_digest,
            payload_digest,
            state,
            resolved_at
        ],
    )
}

pub(super) fn insert_lineage(
    connection: &rusqlite::Connection,
    child_project_id: &str,
) -> rusqlite::Result<usize> {
    connection.execute(
        "INSERT INTO chat_lineage(
           child_project_id,child_chat_id,parent_project_id,parent_chat_id,
           source_session_id,source_turn_id,relation,external_operation_id,created_at
         ) VALUES (?1,'chat-child','project-a','chat-a','session-a','turn-a',
                   'nativeFork','original',1)",
        [child_project_id],
    )
}

pub(super) fn require_ops(steps: &[Value], expected: &[&str]) {
    let actual = steps
        .iter()
        .map(|step| step["op"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

pub(super) fn integer(step: &Value, field: &str) -> i64 {
    step[field].as_i64().unwrap()
}

pub(super) fn string<'a>(step: &'a Value, field: &str) -> &'a str {
    step[field].as_str().unwrap()
}

pub(super) fn valid_digest(value: &str) -> &str {
    assert_eq!(value.len(), 64);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    );
    value
}

pub(super) fn constraint(result: &rusqlite::Result<usize>) -> &'static str {
    match result {
        Err(Error::SqliteFailure(error, _)) => match error.extended_code {
            ffi::SQLITE_CONSTRAINT_FOREIGNKEY => "foreign-key",
            ffi::SQLITE_CONSTRAINT_PRIMARYKEY | ffi::SQLITE_CONSTRAINT_UNIQUE => "unique",
            ffi::SQLITE_CONSTRAINT_CHECK => "check",
            ffi::SQLITE_CONSTRAINT_TRIGGER => "trigger",
            code => panic!("unexpected SQLite constraint code {code}"),
        },
        Ok(_) => panic!("fixture expected SQLite to reject the statement"),
        Err(error) => panic!("unexpected SQLite error {error}"),
    }
}
