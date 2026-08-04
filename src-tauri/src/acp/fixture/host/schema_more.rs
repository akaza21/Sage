use rusqlite::params;
use serde_json::{Value, json};

use super::schema_common::{
    constraint, insert_lineage, insert_request, integer, require_ops, string, valid_digest,
};
use super::schema_seed::{
    DIGEST_A, SchemaFixture, count, insert_activity, insert_chat, insert_epoch, insert_item,
    insert_project, insert_turn,
};

pub(super) fn delivery_regression(steps: &[Value]) -> Value {
    require_ops(steps, &["advanceDelivery", "advanceDelivery"]);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    for state in steps[0]["states"].as_array().unwrap().iter().skip(1) {
        connection
            .execute(
                "UPDATE turns SET delivery_state=?1 WHERE id='turn-a'",
                [state.as_str().unwrap()],
            )
            .unwrap();
    }
    let result = connection.execute(
        "UPDATE turns SET delivery_state=?1 WHERE id='turn-a'",
        [steps[1]["states"][0].as_str().unwrap()],
    );
    let retained: String = connection
        .query_row(
            "SELECT delivery_state FROM turns WHERE id='turn-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "deliveryState": retained})
}

pub(super) fn terminal_turn_mutation(steps: &[Value]) -> Value {
    require_ops(steps, &["insertTerminalTurn", "updateTurn"]);
    let fixture = SchemaFixture::session();
    let connection = fixture.database.connection();
    insert_epoch(&connection, 1, "exited", Some(1));
    insert_turn(
        &connection,
        "turn-a",
        1,
        0,
        string(&steps[0], "state"),
        "correlated",
        Some(integer(&steps[0], "terminalAt")),
    );
    let result = connection.execute(
        "UPDATE turns SET terminal_at=?1 WHERE id='turn-a'",
        [integer(&steps[1], "terminalAt")],
    );
    let retained: i64 = connection
        .query_row(
            "SELECT terminal_at FROM turns WHERE id='turn-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "terminalAt": retained})
}

pub(super) fn resolved_request_mutation(steps: &[Value]) -> Value {
    require_ops(steps, &["insertResolvedRequest", "updateServerRequest"]);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    insert_request(
        &connection,
        "request-a",
        2,
        DIGEST_A,
        valid_digest(string(&steps[0], "payloadDigest")),
        "resolvedDeclined",
        Some(integer(&steps[0], "resolvedAt")),
    )
    .unwrap();
    let result = connection.execute(
        "UPDATE server_requests SET payload_digest=?1,resolved_at=?2 WHERE id='request-a'",
        params![
            valid_digest(string(&steps[1], "payloadDigest")),
            integer(&steps[1], "resolvedAt")
        ],
    );
    let retained: (String, i64) = connection
        .query_row(
            "SELECT payload_digest,resolved_at FROM server_requests WHERE id='request-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "payloadDigest": retained.0,
           "resolvedAt": retained.1})
}

pub(super) fn resolved_time_parity(steps: &[Value]) -> Value {
    require_ops(steps, &["insertServerRequest"]);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "server_requests");
    let result = insert_request(
        &connection,
        "request-a",
        2,
        DIGEST_A,
        DIGEST_A,
        string(&steps[0], "state"),
        Some(integer(&steps[0], "resolvedAt")),
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "server_requests") - before})
}

pub(super) fn external_item_conflict(steps: &[Value]) -> Value {
    require_ops(steps, &["insertItem", "insertItem"]);
    assert_eq!(integer(&steps[0], "runtimeEpoch"), 1);
    assert_eq!(integer(&steps[1], "runtimeEpoch"), 1);
    assert_eq!(string(&steps[0], "kind"), "text");
    assert_eq!(string(&steps[1], "kind"), "text");
    assert_eq!(
        string(&steps[0], "externalItemId"),
        string(&steps[1], "externalItemId")
    );
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "items");
    let external_id = string(&steps[0], "externalItemId");
    insert_item(
        &connection,
        "external-1",
        "turn-old",
        1,
        1,
        Some(external_id),
    );
    let result = connection.execute(
        "INSERT INTO items(
           id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,
           lifecycle,external_item_id,payload_json,payload_bytes,payload_digest,
           created_at,updated_at,terminal_at
         ) VALUES ('external-2','project-a','chat-a','session-a','turn-old',1,2,
                   'primary','text','completed',?1,'{}',2,?2,1,1,2)",
        params![external_id, DIGEST_A],
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "items") - before})
}

pub(super) fn lineage_mutation_delete(steps: &[Value]) -> Value {
    require_ops(
        steps,
        &[
            "insertChatLineage",
            "updateChatLineage",
            "deleteChatLineage",
        ],
    );
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    insert_chat(&connection, "project-a", "chat-child", 1);
    insert_lineage(&connection, "project-a").unwrap();
    let updated = connection.execute(
        "UPDATE chat_lineage SET external_operation_id='changed' WHERE child_chat_id='chat-child'",
        [],
    );
    let deleted = connection.execute(
        "DELETE FROM chat_lineage WHERE child_chat_id='chat-child'",
        [],
    );
    let retained: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM chat_lineage WHERE child_chat_id='chat-child' AND external_operation_id='original'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"updateAccepted": updated.is_ok(), "deleteAccepted": deleted.is_ok(),
           "originalPreserved": retained == 1})
}

pub(super) fn activity_identity_mutation(steps: &[Value]) -> Value {
    require_ops(steps, &["insertActivity", "updateActivity"]);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    insert_activity(
        &connection,
        "activity-a",
        "turn-a",
        2,
        "item-a",
        string(&steps[0], "externalActivityId"),
        "observed",
        0,
        None,
    )
    .unwrap();
    let result = connection.execute(
        "UPDATE session_activities SET external_activity_id=?1 WHERE id='activity-a'",
        [string(&steps[1], "externalActivityId")],
    );
    let retained: String = connection
        .query_row(
            "SELECT external_activity_id FROM session_activities WHERE id='activity-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "externalActivityId": retained})
}

pub(super) fn foreign_parent_item(steps: &[Value]) -> Value {
    require_ops(steps, &["insertActivity"]);
    assert_eq!(string(&steps[0], "parentItemOwner"), "foreign-turn");
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    let before = count(&connection, "session_activities");
    let result = insert_activity(
        &connection,
        "activity-a",
        "turn-a",
        2,
        "item-old",
        "external-a",
        "observed",
        0,
        None,
    );
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": count(&connection, "session_activities") - before})
}

pub(super) fn conflicting_activity_terminal(steps: &[Value]) -> Value {
    require_ops(steps, &["terminalizeActivity", "terminalizeActivity"]);
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    insert_activity(
        &connection,
        "activity-a",
        "turn-a",
        2,
        "item-a",
        "external-a",
        "active",
        1,
        None,
    )
    .unwrap();
    connection
        .execute(
            "UPDATE session_activities SET lifecycle=?1,revision=?2,terminal_at=2 WHERE id='activity-a'",
            params![string(&steps[0], "state"), integer(&steps[0], "revision")],
        )
        .unwrap();
    let result = connection.execute(
        "UPDATE session_activities SET lifecycle=?1,revision=?2,terminal_at=3 WHERE id='activity-a'",
        params![string(&steps[1], "state"), integer(&steps[1], "revision")],
    );
    let retained: (String, i64) = connection
        .query_row(
            "SELECT lifecycle,revision FROM session_activities WHERE id='activity-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    json!({"accepted": result.is_ok(), "firstTerminal": retained.0, "revision": retained.1})
}

pub(super) fn head_update_terminal_session(steps: &[Value]) -> Value {
    require_ops(
        steps,
        &["insertChatHead", "terminalizeSession", "updateChatHead"],
    );
    assert_eq!(string(&steps[0], "sessionState"), "ready");
    assert_eq!(string(&steps[1], "via"), "closing");
    assert_eq!(steps[1]["atomic"].as_bool(), Some(true));
    assert_eq!(steps[2]["sameSession"].as_bool(), Some(true));
    let fixture = SchemaFixture::session();
    let mut connection = fixture.database.connection();
    connection
        .execute(
            "INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at)
             VALUES ('project-a','chat-a','session-a',0,1)",
            [],
        )
        .unwrap();
    let terminal = (|| -> rusqlite::Result<()> {
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE agent_sessions SET state='closing' WHERE id='session-a'",
            [],
        )?;
        transaction.execute(
            "UPDATE agent_sessions SET state=?1,residency='cold',terminal_at=2 WHERE id='session-a'",
            [string(&steps[1], "state")],
        )?;
        transaction.commit()
    })();
    let update = connection.execute(
        "UPDATE chat_session_heads SET session_id=session_id WHERE chat_id='chat-a'",
        [],
    );
    let session_state: String = connection
        .query_row(
            "SELECT state FROM agent_sessions WHERE id='session-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let head_preserved: bool = connection
        .query_row(
            "SELECT session_id='session-a' FROM chat_session_heads WHERE chat_id='chat-a'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    json!({"accepted": terminal.is_ok() && update.is_ok(), "sqlite": constraint(&update),
           "replacementRequired": terminal.is_err() && update.is_err() && session_state == "ready",
           "sessionState": session_state, "headPreserved": head_preserved})
}

pub(super) fn lineage_cross_project(steps: &[Value]) -> Value {
    require_ops(steps, &["insertChatLineage"]);
    assert_eq!(string(&steps[0], "parentProject"), "project-a");
    let fixture = SchemaFixture::graph();
    let connection = fixture.database.connection();
    insert_project(&connection, string(&steps[0], "childProject"));
    insert_chat(
        &connection,
        string(&steps[0], "childProject"),
        "chat-child",
        0,
    );
    let before: i64 = connection
        .query_row("SELECT COUNT(*) FROM chat_lineage", [], |row| row.get(0))
        .unwrap();
    let result = insert_lineage(&connection, string(&steps[0], "childProject"));
    let after: i64 = connection
        .query_row("SELECT COUNT(*) FROM chat_lineage", [], |row| row.get(0))
        .unwrap();
    json!({"accepted": result.is_ok(), "sqlite": constraint(&result),
           "durableRowsAdded": after - before})
}
