use super::*;

const MAPPED_CALL: &str = "{\"kind\":\"toolCall\",\"payload\":{\"category\":\"execute\",\"label\":\"cargo test --lib\",\"inputSummary\":\"cargo test --lib\",\"outputSummary\":null,\"locations\":[{\"kind\":\"resource\",\"resource\":{\"rootId\":\"root\",\"relativePath\":\"src/main.rs\"}}],\"commandActivity\":\"declaredTest\",\"execution\":{\"cwd\":null,\"exitCode\":null,\"outputTail\":null,\"outputTruncated\":false}}}";

const BARE_UPDATE: &str = "{\"kind\":\"toolUpdateResult\",\"payload\":{\"category\":\"execute\",\"label\":\"\",\"outputSummary\":\"1 passed\",\"locations\":[],\"commandActivity\":\"ordinary\",\"execution\":{\"cwd\":null,\"exitCode\":0,\"outputTail\":null,\"outputTruncated\":false}}}";

const FOREIGN_UPDATE: &str = "{\"kind\":\"text\",\"payload\":{\"text\":\"not a tool payload\"}}";

fn call_item<'a>(
    id: &'a str,
    ordinal: i64,
    external_item_id: &'a str,
    payload: &'a str,
) -> SeedItem<'a> {
    SeedItem {
        id,
        ordinal,
        kind: "toolCall",
        lifecycle: "streaming",
        tool_state: "inProgress",
        external_item_id,
        payload,
        terminal_at: None,
    }
}

fn update_item<'a>(
    id: &'a str,
    ordinal: i64,
    external_item_id: &'a str,
    payload: &'a str,
) -> SeedItem<'a> {
    SeedItem {
        id,
        ordinal,
        kind: "toolUpdateResult",
        lifecycle: "completed",
        tool_state: "completed",
        external_item_id,
        payload,
        terminal_at: Some(5),
    }
}

fn paired(call: &str, update: &str) -> Connection {
    let connection = base();
    insert_item(&connection, call_item("call-a", 0, "ext-a", call));
    insert_item(&connection, update_item("update-a", 1, "ext-a", update));
    seal_usage(&connection);
    connection
}

fn near_limit_text() -> String {
    "\"".repeat(65_536)
}

fn tool_envelope(kind: &str, input_summary: Value, output_summary: Value) -> String {
    serde_json::json!({
        "kind": kind,
        "payload": {
            "category": "execute",
            "label": "cargo test --lib",
            "inputSummary": input_summary,
            "outputSummary": output_summary,
            "locations": [],
            "commandActivity": "ordinary",
            "execution": {
                "cwd": Value::Null,
                "exitCode": Value::Null,
                "outputTail": Value::Null,
                "outputTruncated": false
            }
        }
    })
    .to_string()
}

#[test]
fn a_call_only_field_survives_an_update_that_does_not_carry_it() {
    let mut connection = paired(MAPPED_CALL, BARE_UPDATE);
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let folded = payload(&connection, "call-a");
    assert_eq!(folded["payload"]["label"], "cargo test --lib");
    assert_eq!(folded["payload"]["commandActivity"], "declaredTest");
    assert_eq!(
        folded["payload"]["locations"][0]["resource"]["relativePath"],
        "src/main.rs"
    );
    assert_eq!(folded["payload"]["inputSummary"], "cargo test --lib");
    assert_eq!(folded["payload"]["outputSummary"], "1 passed");
    assert_eq!(folded["payload"]["execution"]["exitCode"], 0);
}

#[test]
fn a_terminal_update_can_replace_a_nonterminal_calls_presentation() {
    let update = serde_json::json!({
        "kind": "toolUpdateResult",
        "payload": {
            "category": "fetch",
            "label": "Updated presentation",
            "outputSummary": "8 passed"
        }
    })
    .to_string();
    let mut connection = paired(CALL, &update);
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let folded = payload(&connection, "call-a");
    assert_eq!(folded["payload"]["category"], "fetch");
    assert_eq!(folded["payload"]["label"], "Updated presentation");
    assert_eq!(folded["payload"]["outputSummary"], "8 passed");
}

#[test]
fn a_merge_over_the_item_limit_fails_the_upgrade_and_keeps_both_payloads() {
    let text = near_limit_text();
    let call = tool_envelope("toolCall", Value::String(text.clone()), Value::Null);
    let update = tool_envelope("toolUpdateResult", Value::Null, Value::String(text));
    let mut connection = paired(&call, &update);
    let error = schema_upgrade::apply_v11(&mut connection).unwrap_err();
    assert!(
        matches!(error, AppError::ResourceLimit(_)),
        "expected a capacity error, got {error:?}"
    );
    assert_eq!(payload(&connection, "call-a"), parsed(&call));
    assert_eq!(payload(&connection, "update-a"), parsed(&update));
}

fn parsed(payload: &str) -> Value {
    serde_json::from_str(payload).unwrap()
}

#[test]
fn a_legacy_update_that_is_not_a_tool_payload_fails_the_upgrade() {
    let connection = base();
    insert_item(
        &connection,
        update_item("update-a", 0, "ext-a", FOREIGN_UPDATE),
    );
    seal_usage(&connection);
    let mut connection = connection;
    let error = schema_upgrade::apply_v11(&mut connection).unwrap_err();
    assert!(
        matches!(error, AppError::InvalidRequest(_)),
        "expected a decode error, got {error:?}"
    );
    let kind: String = connection
        .query_row("SELECT kind FROM items WHERE id='update-a'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kind, "toolUpdateResult");
}

fn seeded_pairs(count: i64, call: &str, update: &str) -> Connection {
    let connection = base();
    for index in 0..count {
        let external = format!("ext-{index}");
        insert_item(
            &connection,
            call_item(&format!("call-{index}"), index * 2, &external, call),
        );
        insert_item(
            &connection,
            update_item(&format!("update-{index}"), index * 2 + 1, &external, update),
        );
    }
    seal_usage(&connection);
    connection
}

fn page(connection: &mut Connection, cursor: &str) -> (Vec<String>, i64) {
    let transaction = connection.transaction().unwrap();
    tool_result_fold::page_for_test(&transaction, cursor).unwrap()
}

#[test]
fn a_legacy_update_page_stops_at_its_row_bound() {
    let mut connection = seeded_pairs(300, CALL, UPDATE);
    assert_eq!(page(&mut connection, "").0.len(), 128);
}

#[test]
fn a_legacy_update_page_stops_below_its_byte_bound() {
    let bulk = "\"".repeat(60_000);
    let call = tool_envelope("toolCall", Value::String(bulk.clone()), Value::Null);
    let update = tool_envelope("toolUpdateResult", Value::Null, Value::String(bulk));
    let mut connection = seeded_pairs(32, &call, &update);
    let (ids, bytes) = page(&mut connection, "");
    let pair = (call.len() + update.len()) as i64;
    assert!(
        (1..32).contains(&ids.len()),
        "unbounded page of {} updates",
        ids.len()
    );
    assert!(
        bytes <= tool_result_fold::PAGE_BYTES_BOUND,
        "page retained {bytes} bytes over its bound"
    );
    assert!(
        bytes + pair > tool_result_fold::PAGE_BYTES_BOUND,
        "page stopped early at {bytes} bytes"
    );
}

#[test]
fn a_legacy_update_page_resumes_after_its_cursor() {
    let mut connection = seeded_pairs(300, CALL, UPDATE);
    let (first, _) = page(&mut connection, "");
    assert!(
        first.windows(2).all(|pair| pair[0] < pair[1]),
        "page is not ordered by id: {first:?}"
    );
    let cursor = first.last().unwrap().clone();
    let (next, _) = page(&mut connection, &cursor);
    assert!(
        next.iter().all(|id| id.as_str() > cursor.as_str()),
        "page repeated rows at or before {cursor}"
    );
}

#[test]
fn a_legacy_update_page_seeks_the_items_primary_key() {
    let mut connection = seeded_pairs(4, CALL, UPDATE);
    let transaction = connection.transaction().unwrap();
    let plan = tool_result_fold::page_plan_for_test(&transaction).unwrap();
    assert!(
        !plan.contains("TEMP B-TREE"),
        "page query sorts every page: {plan}"
    );
    assert!(
        plan.contains("sqlite_autoindex_items_1"),
        "page query does not seek the items primary key: {plan}"
    );
}

#[test]
fn every_update_beyond_one_page_still_folds() {
    let mut connection = seeded_pairs(300, CALL, UPDATE);
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let remaining: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE kind='toolUpdateResult'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    assert_eq!(item_count(&connection), 300);
    assert_ledger(&connection);
}

fn assert_ledger(connection: &Connection) {
    let bytes = stored_bytes(connection);
    let count = item_count(connection);
    let session: (i64, i64, i64) = connection
        .query_row(
            "SELECT item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id='session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(session, (count, bytes, bytes));
    assert_eq!(inconsistent_rows(connection), 0);
}
