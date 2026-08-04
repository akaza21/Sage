use rusqlite::params;
use serde_json::{Value, json};

use super::schema_common::{
    constraint, insert_request, integer, require_ops, string, valid_digest,
};
use super::schema_seed::{
    DIGEST_A, DIGEST_B, SchemaFixture, count, insert_activity, insert_epoch, insert_session,
};
use crate::acp::fixture::generated_cases::FrozenCase;

pub(in crate::acp::fixture) const CASE_IDS: &[&str] = &[
    "runtime-epoch-item-mismatch",
    "runtime-epoch-request-mismatch",
    "server-request-binding-digest-mismatch",
    "runtime-epoch-activity-mismatch",
    "runtime-epoch-reuse",
    "runtime-epoch-delete-reuse",
    "agent-session-nonterminal-chat-conflict",
    "agent-session-binding-mutation",
    "terminal-agent-session-mutation",
    "turn-delivery-regression",
    "terminal-turn-mutation",
    "resolved-request-mutation",
    "resolved-request-time-parity",
    "external-item-id-epoch-kind-conflict",
    "chat-lineage-mutation-delete",
    "activity-identity-mutation",
    "native-activity-foreign-parent-item",
    "native-activity-conflicting-terminal",
    "chat-head-update-terminal-session",
    "chat-lineage-cross-project",
];

pub(in crate::acp::fixture) fn execute(case: &FrozenCase) -> Value {
    let steps: Vec<Value> = serde_json::from_str(case.steps_json).unwrap();
    match case.id {
        "runtime-epoch-item-mismatch" => epoch_item_mismatch(&steps),
        "runtime-epoch-request-mismatch" => epoch_request_mismatch(&steps),
        "server-request-binding-digest-mismatch" => binding_mismatch(&steps),
        "runtime-epoch-activity-mismatch" => epoch_activity_mismatch(&steps),
        "runtime-epoch-reuse" => epoch_reuse(&steps),
        "runtime-epoch-delete-reuse" => epoch_delete_reuse(&steps),
        "agent-session-nonterminal-chat-conflict" => nonterminal_session_conflict(&steps),
        "agent-session-binding-mutation" => binding_mutation(&steps),
        "terminal-agent-session-mutation" => terminal_session_mutation(&steps),
        "turn-delivery-regression" => super::schema_more::delivery_regression(&steps),
        "terminal-turn-mutation" => super::schema_more::terminal_turn_mutation(&steps),
        "resolved-request-mutation" => super::schema_more::resolved_request_mutation(&steps),
        "resolved-request-time-parity" => super::schema_more::resolved_time_parity(&steps),
        "external-item-id-epoch-kind-conflict" => {
            super::schema_more::external_item_conflict(&steps)
        }
        "chat-lineage-mutation-delete" => super::schema_more::lineage_mutation_delete(&steps),
        "activity-identity-mutation" => super::schema_more::activity_identity_mutation(&steps),
        "native-activity-foreign-parent-item" => super::schema_more::foreign_parent_item(&steps),
        "native-activity-conflicting-terminal" => {
            super::schema_more::conflicting_activity_terminal(&steps)
        }
        "chat-head-update-terminal-session" => {
            super::schema_more::head_update_terminal_session(&steps)
        }
        "chat-lineage-cross-project" => super::schema_more::lineage_cross_project(&steps),
        _ => panic!("{} is not a frozen schema fixture", case.id),
    }
}

fn epoch_item_mismatch(steps: &[Value]) -> Value {
    require_ops(steps, &["insertItem"]);
    assert_eq!(integer(&steps[0], "sessionEpoch"), 2);
    assert_eq!(integer(&steps[0], "turnEpoch"), 2);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "items");
    let result = connection.execute(
        "INSERT INTO items(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,
           origin,kind,lifecycle,payload_json,payload_bytes,payload_digest,
           created_at,updated_at,terminal_at
         ) VALUES ('mismatch','project-a','chat-a','session-a','turn-a',?1,2,
                   'primary','text','completed','{}',2,?2,1,1,2)",
        params![integer(&steps[0], "itemEpoch"), DIGEST_A],
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "items") - before})
}

fn epoch_request_mismatch(steps: &[Value]) -> Value {
    require_ops(steps, &["insertServerRequest"]);
    assert_eq!(integer(&steps[0], "turnEpoch"), 2);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "server_requests");
    let result = insert_request(
        &connection,
        "request-a",
        integer(&steps[0], "requestEpoch"),
        DIGEST_A,
        DIGEST_A,
        "received",
        None,
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "server_requests") - before})
}

fn binding_mismatch(steps: &[Value]) -> Value {
    require_ops(steps, &["insertServerRequest"]);
    let session_digest = valid_digest(string(&steps[0], "sessionBindingDigest"));
    let fixture = SchemaFixture::graph_with_binding(session_digest);
    let connection = fixture.database.connection();
    let before = count(&connection, "server_requests");
    let result = insert_request(
        &connection,
        "request-a",
        2,
        valid_digest(string(&steps[0], "requestBindingDigest")),
        DIGEST_A,
        "received",
        None,
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "server_requests") - before})
}

fn epoch_activity_mismatch(steps: &[Value]) -> Value {
    require_ops(steps, &["insertActivity"]);
    assert_eq!(integer(&steps[0], "turnEpoch"), 2);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "session_activities");
    let result = insert_activity(
        &connection,
        "activity-a",
        "turn-a",
        integer(&steps[0], "activityEpoch"),
        "item-a",
        "external-a",
        "observed",
        0,
        None,
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "session_activities") - before})
}

fn epoch_reuse(steps: &[Value]) -> Value {
    require_ops(steps, &["insertRuntimeEpoch", "insertRuntimeEpoch"]);
    let fixture = SchemaFixture::session();
    let connection = fixture.database.connection();
    let epoch = integer(&steps[0], "runtimeEpoch");
    assert_eq!(integer(&steps[1], "runtimeEpoch"), epoch);
    insert_epoch(&connection, epoch, "running", None);
    let result = connection.execute(
        "INSERT INTO session_runtime_epochs(
           project_id,chat_id,session_id,runtime_epoch,state,started_at
         ) VALUES ('project-a','chat-a','session-a',?1,'running',2)",
        [epoch],
    );
    let preserved = connection
        .query_row(
            "SELECT started_at=1 FROM session_runtime_epochs
             WHERE session_id='session-a' AND runtime_epoch=?1",
            [epoch],
            |row| row.get::<_, bool>(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "firstEpochPreserved": preserved})
}

fn epoch_delete_reuse(steps: &[Value]) -> Value {
    require_ops(
        steps,
        &[
            "insertRuntimeEpoch",
            "deleteRuntimeEpoch",
            "insertRuntimeEpoch",
        ],
    );
    let fixture = SchemaFixture::session();
    let connection = fixture.database.connection();
    let epoch = integer(&steps[0], "runtimeEpoch");
    assert_eq!(integer(&steps[1], "runtimeEpoch"), epoch);
    assert_eq!(integer(&steps[2], "runtimeEpoch"), epoch);
    insert_epoch(&connection, epoch, "running", None);
    let deleted = connection.execute(
        "DELETE FROM session_runtime_epochs WHERE session_id='session-a' AND runtime_epoch=?1",
        [epoch],
    );
    let reused = connection.execute(
        "INSERT INTO session_runtime_epochs(
           project_id,chat_id,session_id,runtime_epoch,state,started_at
         ) VALUES ('project-a','chat-a','session-a',?1,'running',2)",
        [epoch],
    );
    let preserved: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM session_runtime_epochs
             WHERE session_id='session-a' AND runtime_epoch=?1 AND started_at=1",
            [epoch],
            |row| row.get(0),
        )
        .unwrap();
    json!({"deleteAccepted": deleted.is_ok(), "reuseAccepted": reused.is_ok(),
           "firstEpochPreserved": preserved == 1})
}

fn nonterminal_session_conflict(steps: &[Value]) -> Value {
    require_ops(steps, &["insertAgentSession", "insertAgentSession"]);
    assert_eq!(string(&steps[0], "chatId"), "chat-a");
    assert_eq!(string(&steps[1], "chatId"), "chat-a");
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    insert_session(
        &connection,
        "project-a",
        "chat-a",
        "session-a",
        0,
        string(&steps[0], "state"),
        "hot",
        None,
        DIGEST_A,
    );
    let result = connection.execute(
        "INSERT INTO agent_sessions(
           id,project_id,chat_id,ordinal,binding_id,binding_digest,capabilities_json,
           state,residency,created_at,updated_at
         ) VALUES ('session-b','project-a','chat-a',1,'binding-b',?1,'{}',?2,'cold',1,1)",
        params![DIGEST_B, string(&steps[1], "state")],
    );
    let retained: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM agent_sessions WHERE chat_id='chat-a' AND state NOT IN ('closed','failed','incompatible')",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "nonterminalSessions": retained})
}

fn binding_mutation(steps: &[Value]) -> Value {
    require_ops(steps, &["insertAgentSession", "updateAgentSession"]);
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    insert_session(
        &connection,
        "project-a",
        "chat-a",
        "session-a",
        0,
        "ready",
        "hot",
        None,
        valid_digest(string(&steps[0], "bindingDigest")),
    );
    let result = connection.execute(
        "UPDATE agent_sessions SET binding_digest=?1 WHERE id='session-a'",
        [valid_digest(string(&steps[1], "bindingDigest"))],
    );
    let retained: String = connection
        .query_row(
            "SELECT binding_digest FROM agent_sessions WHERE id='session-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "bindingDigest": retained})
}

fn terminal_session_mutation(steps: &[Value]) -> Value {
    require_ops(steps, &["insertTerminalAgentSession", "updateAgentSession"]);
    let fixture = SchemaFixture::project_chat();
    let connection = fixture.database.connection();
    insert_session(
        &connection,
        "project-a",
        "chat-a",
        "session-a",
        0,
        string(&steps[0], "state"),
        "cold",
        Some(integer(&steps[0], "terminalAt")),
        DIGEST_A,
    );
    let result = connection.execute(
        "UPDATE agent_sessions SET terminal_at=?1 WHERE id='session-a'",
        [integer(&steps[1], "terminalAt")],
    );
    let retained: i64 = connection
        .query_row(
            "SELECT terminal_at FROM agent_sessions WHERE id='session-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "terminalAt": retained})
}
