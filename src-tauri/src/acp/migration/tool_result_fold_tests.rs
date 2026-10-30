use rusqlite::{Connection, params};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::*;
use crate::acp::{
    contract::{SageChatId, SageProjectId},
    store::load_chat_page,
};

const CAPABILITIES: &str = "{\"schemaVersion\":1,\"protocolVersion\":\"acp-1\",\"newSession\":true,\"resumeSession\":false,\"loadSession\":false,\"closeSession\":true,\"prompt\":true,\"cancel\":true,\"nativeSessionFork\":false,\"activeTurnSteer\":false,\"childConversationProjection\":false,\"childTranscriptRead\":false,\"collaborationEventProjection\":false,\"multiRootExtensionSchemaId\":null,\"modelCount\":1,\"configCount\":0}";

const CALL: &str = "{\"kind\":\"toolCall\",\"payload\":{\"category\":\"execute\",\"label\":\"npm test\",\"inputSummary\":\"npm test\",\"outputSummary\":null,\"locations\":[],\"commandActivity\":\"ordinary\",\"execution\":{\"cwd\":null,\"exitCode\":null,\"outputTail\":null,\"outputTruncated\":false}}}";

const UPDATE: &str = "{\"kind\":\"toolUpdateResult\",\"payload\":{\"category\":\"execute\",\"label\":\"npm test\",\"outputSummary\":\"7 passed\",\"locations\":[],\"commandActivity\":\"ordinary\",\"execution\":{\"cwd\":null,\"exitCode\":0,\"outputTail\":\"ok\",\"outputTruncated\":false}}}";

const ORPHAN: &str = "{\"kind\":\"toolUpdateResult\",\"payload\":{\"category\":\"fetch\",\"label\":\"fetch docs\",\"outputSummary\":\"404\",\"locations\":[],\"commandActivity\":\"ordinary\",\"execution\":{\"cwd\":null,\"exitCode\":null,\"outputTail\":null,\"outputTruncated\":false}}}";

const PROVENANCE: &str = "{\"schemaVersion\":1,\"sourceKind\":\"tool\",\"externalKind\":\"tool\",\"extensionSchemaId\":null,\"safeLabel\":null}";

mod fold_cases;

fn base() -> Connection {
    let mut connection = Connection::open_in_memory().unwrap();
    schema_upgrade::build_fresh_v10(&mut connection).unwrap();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project','Project',1,1,1)", []).unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root','project','C:/project','c:/project',0)", []).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat','project',0,'Chat',0,1,1)", []).unwrap();
    connection
        .execute(
            "INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session','project','chat',0,'binding',?1,'external',?2,'active','hot',0,0,0,1,1)",
            params!["a".repeat(64), CAPABILITIES],
        )
        .unwrap();
    connection.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project','chat','session',0,1)", []).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project','chat','session',1,'running',?1,1)", ["b".repeat(64)]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn','project','chat','session',1,0,'intent','running','correlated',0,1,1)", []).unwrap();
    connection
}

fn seeded() -> Connection {
    let connection = base();
    insert_item(
        &connection,
        SeedItem {
            id: "call-a",
            ordinal: 0,
            kind: "toolCall",
            lifecycle: "streaming",
            tool_state: "inProgress",
            external_item_id: "ext-a",
            payload: CALL,
            terminal_at: None,
        },
    );
    insert_item(
        &connection,
        SeedItem {
            id: "update-a",
            ordinal: 1,
            kind: "toolUpdateResult",
            lifecycle: "completed",
            tool_state: "completed",
            external_item_id: "ext-a",
            payload: UPDATE,
            terminal_at: Some(5),
        },
    );
    insert_item(
        &connection,
        SeedItem {
            id: "update-b",
            ordinal: 2,
            kind: "toolUpdateResult",
            lifecycle: "failed",
            tool_state: "failed",
            external_item_id: "ext-b",
            payload: ORPHAN,
            terminal_at: Some(6),
        },
    );
    connection
        .execute(
            "INSERT INTO session_activities(id,project_id,chat_id,session_id,turn_id,runtime_epoch,external_activity_id,activity_kind,parent_activity_id,parent_item_id,role,lifecycle,depth,observed_sequence,provenance_json,revision,created_at,updated_at,terminal_at) VALUES('activity','project','chat','session','turn',1,'external-activity','fork',NULL,'update-a','fork','active',0,1,?1,0,1,1,NULL)",
            [PROVENANCE],
        )
        .unwrap();
    seal_usage(&connection);
    connection
}

fn seal_usage(connection: &Connection) {
    let bytes = stored_bytes(connection);
    let count = item_count(connection);
    connection
        .execute(
            "INSERT OR REPLACE INTO acp_session_usage(project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,fact_bytes,payload_bytes,revision) VALUES('project','chat','session',1,?2,?1,0,0,?1,0)",
            params![bytes, count],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO acp_project_usage(project_id,chat_count,session_count,turn_count,item_count,fact_count,payload_bytes,revision) VALUES('project',1,1,1,?2,0,?1,0)",
            params![bytes, count],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR REPLACE INTO acp_store_usage(singleton,chat_count,session_count,turn_count,item_count,item_bytes,fact_count,fact_bytes,shared_payload_bytes,revision) VALUES(1,1,1,1,?2,?1,0,0,?1,0)",
            params![bytes, count],
        )
        .unwrap();
}

fn item_count(connection: &Connection) -> i64 {
    connection
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .unwrap()
}

struct SeedItem<'a> {
    id: &'a str,
    ordinal: i64,
    kind: &'a str,
    lifecycle: &'a str,
    tool_state: &'a str,
    external_item_id: &'a str,
    payload: &'a str,
    terminal_at: Option<i64>,
}

fn insert_item(connection: &Connection, item: SeedItem<'_>) {
    connection
        .execute(
            "INSERT INTO items(id,project_id,chat_id,session_id,turn_id,runtime_epoch,ordinal,origin,kind,lifecycle,tool_state,external_item_id,payload_json,payload_bytes,payload_digest,revision,created_at,updated_at,terminal_at) VALUES(?1,'project','chat','session','turn',1,?2,'primary',?3,?4,?5,?6,?7,?8,?9,0,1,1,?10)",
            params![
                item.id,
                item.ordinal,
                item.kind,
                item.lifecycle,
                item.tool_state,
                item.external_item_id,
                item.payload,
                item.payload.len() as i64,
                format!("{:x}", Sha256::digest(item.payload.as_bytes())),
                item.terminal_at,
            ],
        )
        .unwrap();
}

fn stored_bytes(connection: &Connection) -> i64 {
    connection
        .query_row(
            "SELECT COALESCE(SUM(payload_bytes),0) FROM items",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn payload(connection: &Connection, id: &str) -> Value {
    let stored: String = connection
        .query_row("SELECT payload_json FROM items WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .unwrap();
    serde_json::from_str(&stored).unwrap()
}

fn inconsistent_rows(connection: &Connection) -> usize {
    let mut statement = connection
        .prepare("SELECT payload_json,payload_bytes,payload_digest FROM items")
        .unwrap();
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    rows.into_iter()
        .filter(|(stored, bytes, digest)| {
            *bytes != stored.len() as i64
                || *digest != format!("{:x}", Sha256::digest(stored.as_bytes()))
        })
        .count()
}

#[test]
fn a_legacy_update_folds_into_its_sibling_call_and_leaves_no_update_kind() {
    let mut connection = seeded();
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let remaining: i64 = connection
        .query_row(
            "SELECT COUNT(*) FROM items WHERE kind='toolUpdateResult'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(remaining, 0);
    let folded = payload(&connection, "call-a");
    assert_eq!(folded["kind"], "toolCall");
    assert_eq!(folded["payload"]["inputSummary"], "npm test");
    assert_eq!(folded["payload"]["outputSummary"], "7 passed");
    assert_eq!(folded["payload"]["execution"]["exitCode"], 0);
    let status: (String, String, Option<i64>) = connection
        .query_row(
            "SELECT lifecycle,tool_state,terminal_at FROM items WHERE id='call-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(
        status,
        ("completed".to_owned(), "completed".to_owned(), Some(5))
    );
    assert_eq!(inconsistent_rows(&connection), 0);
}

#[test]
fn an_orphan_update_becomes_a_call_of_its_own() {
    let mut connection = seeded();
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let kind: String = connection
        .query_row("SELECT kind FROM items WHERE id='update-b'", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(kind, "toolCall");
    let orphan = payload(&connection, "update-b");
    assert_eq!(orphan["kind"], "toolCall");
    assert_eq!(orphan["payload"]["inputSummary"], Value::Null);
    assert_eq!(orphan["payload"]["outputSummary"], "404");
}

#[test]
fn a_stale_nonterminal_update_keeps_a_terminal_call_state_and_presentation() {
    let connection = base();
    let stale_update = serde_json::json!({
        "kind": "toolUpdateResult",
        "payload": {
            "category": "fetch",
            "label": "Wrong replacement",
            "outputSummary": "7 passed",
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
    .to_string();
    insert_item(
        &connection,
        SeedItem {
            id: "call-a",
            ordinal: 0,
            kind: "toolCall",
            lifecycle: "failed",
            tool_state: "failed",
            external_item_id: "ext-a",
            payload: CALL,
            terminal_at: Some(4),
        },
    );
    insert_item(
        &connection,
        SeedItem {
            id: "update-a",
            ordinal: 1,
            kind: "toolUpdateResult",
            lifecycle: "streaming",
            tool_state: "inProgress",
            external_item_id: "ext-a",
            payload: &stale_update,
            terminal_at: None,
        },
    );
    seal_usage(&connection);
    let mut connection = connection;
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let status: (String, String, Option<i64>) = connection
        .query_row(
            "SELECT lifecycle,tool_state,terminal_at FROM items WHERE id='call-a'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(status, ("failed".to_owned(), "failed".to_owned(), Some(4)));
    let folded = payload(&connection, "call-a");
    assert_eq!(folded["payload"]["category"], "execute");
    assert_eq!(folded["payload"]["label"], "npm test");
    assert_eq!(folded["payload"]["outputSummary"], "7 passed");
}

#[test]
fn an_orphan_update_keeps_its_original_turn_ordinal() {
    let mut connection = seeded();
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let ordered = connection
        .prepare("SELECT id,ordinal FROM items WHERE turn_id='turn' ORDER BY ordinal")
        .unwrap()
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        ordered,
        vec![("call-a".to_owned(), 0), ("update-b".to_owned(), 2)]
    );
}

#[test]
fn the_fold_keeps_the_usage_ledger_and_activity_parents_consistent() {
    let mut connection = seeded();
    schema_upgrade::apply_v11(&mut connection).unwrap();
    let bytes = stored_bytes(&connection);
    let count: i64 = connection
        .query_row("SELECT COUNT(*) FROM items", [], |row| row.get(0))
        .unwrap();
    assert_eq!(count, 2);
    let session: (i64, i64, i64) = connection
        .query_row(
            "SELECT item_count,item_bytes,payload_bytes FROM acp_session_usage WHERE session_id='session'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(session, (count, bytes, bytes));
    let store: (i64, i64, i64) = connection
        .query_row(
            "SELECT item_count,item_bytes,shared_payload_bytes FROM acp_store_usage WHERE singleton=1",
            [],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(store, (count, bytes, bytes));
    let parent: String = connection
        .query_row(
            "SELECT parent_item_id FROM session_activities WHERE id='activity'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(parent, "call-a");
    let dangling = connection
        .query_row("PRAGMA foreign_key_check", [], |_| Ok(true))
        .optional()
        .unwrap()
        .unwrap_or(false);
    assert!(!dangling);
}

#[test]
fn the_folded_history_projects_one_tool_call_per_tool() {
    let mut connection = seeded();
    schema_upgrade::apply_v11(&mut connection).unwrap();
    // The page reads the v16 child-epoch and v20 handoff tables; this history predates both.
    connection
        .execute_batch(
            "CREATE TABLE acp_child_thread_transcripts(native_child_thread_id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,updated_at INTEGER);
             CREATE TABLE acp_context_handoffs(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,target_session TEXT,turn_id TEXT,state TEXT,projection_revision INTEGER,plan_json TEXT);",
        )
        .unwrap();
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project").unwrap(),
        SageChatId::parse("chat").unwrap(),
        None,
    )
    .unwrap();
    let turn = page.turns.as_slice().first().unwrap();
    let calls = turn
        .items
        .as_slice()
        .iter()
        .filter(|item| {
            matches!(
                item.content,
                crate::acp::projection::ItemContent::ToolCall { .. }
            )
        })
        .count();
    assert_eq!(turn.items.as_slice().len(), 2);
    assert_eq!(calls, 2);
    serde_json::to_value(&page).unwrap();
}
