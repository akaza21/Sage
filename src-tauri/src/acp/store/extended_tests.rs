use rusqlite::{Connection, params};

use super::*;
use crate::acp::{
    contract::{ClientIntentId, RequiredText, SchemaVersionV1},
    intent::{
        BindingId, CapabilitySnapshotV1, ExternalId, ModelId, NormalizedPayload, PageCursor,
        ToolCategory,
    },
    lifecycle::SessionState,
    runtime::permissions::{
        DeclineTransport, PermissionDisposition, PermissionError, PermissionIntent,
        decline_permission, decline_written,
    },
};

pub(super) fn setup() -> Connection {
    let connection = Connection::open_in_memory().unwrap();
    connection.execute_batch(
        "CREATE TABLE project_roots(id TEXT PRIMARY KEY,project_id TEXT,absolute_path TEXT,canonical_key TEXT,ordinal INTEGER);
         CREATE TABLE chats(id TEXT PRIMARY KEY,project_id TEXT,ordinal INTEGER,title TEXT,projection_revision INTEGER,created_at INTEGER,updated_at INTEGER);
         CREATE TABLE chat_lineage(child_project_id TEXT,child_chat_id TEXT PRIMARY KEY,parent_project_id TEXT,parent_chat_id TEXT,source_turn_id TEXT,relation TEXT);
         CREATE TABLE acp_child_thread_transcripts(native_child_thread_id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,updated_at INTEGER);
         CREATE TABLE agent_sessions(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,ordinal INTEGER,binding_id TEXT,binding_digest TEXT,harness_version TEXT,model_id TEXT,external_session_id TEXT,capabilities_json TEXT,state TEXT,residency TEXT,legacy INTEGER,revision INTEGER,projection_revision INTEGER,created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE chat_session_heads(project_id TEXT,chat_id TEXT PRIMARY KEY,session_id TEXT UNIQUE,installed_revision INTEGER,installed_at INTEGER);
         CREATE TABLE session_runtime_epochs(project_id TEXT,chat_id TEXT,session_id TEXT,runtime_epoch INTEGER,state TEXT,artifact_digest TEXT,started_at INTEGER,terminal_at INTEGER,exit_kind TEXT,PRIMARY KEY(session_id,runtime_epoch));
         CREATE TABLE turns(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,session_id TEXT,runtime_epoch INTEGER,ordinal INTEGER,client_intent_id TEXT,state TEXT,delivery_state TEXT,revision INTEGER,created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE items(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,ordinal INTEGER,origin TEXT,activity_id TEXT,kind TEXT,lifecycle TEXT,tool_state TEXT,external_item_id TEXT,payload_json TEXT,payload_bytes INTEGER,payload_digest TEXT,revision INTEGER,created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE session_activities(id TEXT,project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,external_activity_id TEXT,activity_kind TEXT,parent_activity_id TEXT,parent_item_id TEXT,role TEXT,lifecycle TEXT,depth INTEGER,observed_sequence INTEGER,provenance_json TEXT,revision INTEGER,created_at INTEGER,updated_at INTEGER,terminal_at INTEGER);
         CREATE TABLE server_requests(id TEXT PRIMARY KEY,project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,binding_digest TEXT,external_request_id TEXT,kind TEXT,payload_digest_version INTEGER,payload_digest TEXT,state TEXT,normalized_payload_json TEXT,revision INTEGER,created_at INTEGER,updated_at INTEGER,resolved_at INTEGER);
         CREATE TABLE session_facts(project_id TEXT,chat_id TEXT,session_id TEXT,sequence INTEGER,turn_id TEXT,kind TEXT,payload_json TEXT,payload_bytes INTEGER,payload_digest TEXT,projection_revision INTEGER,created_at INTEGER,PRIMARY KEY(session_id,sequence));
         CREATE TABLE acp_session_usage(project_id TEXT,chat_id TEXT,session_id TEXT PRIMARY KEY,turn_count INTEGER,item_count INTEGER,item_bytes INTEGER,fact_count INTEGER,fact_bytes INTEGER,payload_bytes INTEGER,revision INTEGER);
         CREATE TABLE acp_project_usage(project_id TEXT PRIMARY KEY,chat_count INTEGER,session_count INTEGER,turn_count INTEGER,item_count INTEGER,fact_count INTEGER,payload_bytes INTEGER,revision INTEGER);
         CREATE TABLE acp_store_usage(singleton INTEGER PRIMARY KEY,chat_count INTEGER,session_count INTEGER,turn_count INTEGER,item_count INTEGER,item_bytes INTEGER,fact_count INTEGER,fact_bytes INTEGER,shared_payload_bytes INTEGER,revision INTEGER);
         INSERT INTO project_roots VALUES('root-one','project-one','C:/project','c:/project',0);
         INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-one','project-one',0,'Chat',0,0,0);
         INSERT INTO acp_project_usage VALUES('project-one',1,0,0,0,0,0,0);
         INSERT INTO acp_store_usage VALUES(1,1,0,0,0,0,0,0,0,0);",
    ).unwrap();
    connection
        .execute_batch(
            "CREATE UNIQUE INDEX snapshot_fixture_item_owner
         ON items(project_id,chat_id,session_id,turn_id,id,runtime_epoch);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/010_acp_file_change_snapshots.sql"
        ))
        .unwrap();
    // The handmade sessions table lacks the owner key v21 references.
    connection
        .execute_batch(
            "CREATE UNIQUE INDEX fixture_session_owner ON agent_sessions(project_id,chat_id,id);",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/021_acp_session_scope.sql"
        ))
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE acp_turn_usage_snapshots(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,input_tokens INTEGER,output_tokens INTEGER,context_window INTEGER,revision INTEGER,created_at INTEGER,updated_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));",
        )
        .unwrap();
    connection
        .execute_batch(
            "CREATE TABLE acp_turn_terminal_reasons(project_id TEXT,chat_id TEXT,session_id TEXT,turn_id TEXT,runtime_epoch INTEGER,kind TEXT,message TEXT,remediation TEXT,created_at INTEGER,PRIMARY KEY(session_id,runtime_epoch,turn_id));",
        )
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/019_turn_workspace_changes.sql"
        ))
        .unwrap();
    connection
        .execute_batch(include_str!(
            "../../../migrations/020_chat_title_provenance.sql"
        ))
        .unwrap();
    connection
}

pub(super) fn capabilities() -> String {
    serde_json::to_string(&CapabilitySnapshotV1 {
        schema_version: SchemaVersionV1,
        protocol_version: RequiredText::new("protocol", "fixture-v1").unwrap(),
        new_session: true,
        resume_session: false,
        load_session: false,
        close_session: true,
        prompt: true,
        cancel: true,
        native_session_fork: false,
        active_turn_steer: false,
        child_conversation_projection: false,
        child_transcript_read: false,
        collaboration_event_projection: false,
        multi_root_extension_schema_id: None,
        model_count: 1,
        prompt_resources: false,
        config_count: 0,
    })
    .unwrap()
}

pub(super) fn start(expected: u64, intent: &str) -> StartSessionCommand {
    StartSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        expected_projection_revision: ProjectionRevision::new(expected).unwrap(),
        client_intent_id: ClientIntentId::parse(intent).unwrap(),
        binding_id: BindingId::new("binding", "fixture").unwrap(),
        binding_digest: "a".repeat(64),
        harness_version: Some(crate::acp::contract::SafeText::new("version", "1.0.0").unwrap()),
        model_id: Some(ModelId::new("model", "fixture-model").unwrap()),
        now_millis: 10,
    }
}

#[test]
fn session_start_activate_close_is_atomic_idempotent_and_monotonic() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-one")).unwrap();
    assert_eq!(started.state, SessionState::Created);
    let duplicate = start_session(&mut connection, start(0, "start-one")).unwrap();
    assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
    let mut conflicting = start(0, "start-one");
    conflicting.model_id = Some(ModelId::new("model", "other").unwrap());
    assert!(matches!(
        start_session(&mut connection, conflicting),
        Err(StoreError::Conflict {
            resource: "clientIntent"
        })
    ));
    let activation = ActivateSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        expected_projection_revision: ProjectionRevision::new(1).unwrap(),
        client_intent_id: ClientIntentId::parse("activate-one").unwrap(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        artifact_digest: "d".repeat(64),
        now_millis: 20,
    };
    let activated = activate_session(&mut connection, activation.clone()).unwrap();
    assert_eq!(
        (activated.state, activated.runtime_epoch),
        (SessionState::Starting, Some(RuntimeEpoch::new(1).unwrap()))
    );
    assert_eq!(
        activate_session(&mut connection, activation)
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    let completion = CompleteActivationCommand {
        binding_change: None,
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        binding_digest: "a".repeat(64),
        artifact_digest: "d".repeat(64),
        external_session_id: ExternalId::new("external", "harness-session").unwrap(),
        capabilities_json: capabilities(),
        now_millis: 25,
    };
    assert_eq!(
        complete_activation(&mut connection, completion.clone())
            .unwrap()
            .state,
        SessionState::Ready
    );
    assert_eq!(
        complete_activation(&mut connection, completion)
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    let close = CloseSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        expected_projection_revision: ProjectionRevision::new(3).unwrap(),
        client_intent_id: ClientIntentId::parse("close-one").unwrap(),
        session_id: started.session_id,
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        now_millis: 30,
    };
    assert_eq!(
        close_session(&mut connection, close.clone()).unwrap().state,
        SessionState::Closing
    );
    assert_eq!(
        close_session(&mut connection, close.clone())
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    assert_eq!(
        finalize_close_after_zero_process(
            &mut connection,
            FinalizeCloseCommand {
                request: close.clone()
            }
        )
        .unwrap()
        .state,
        SessionState::Closed
    );
    assert_eq!(
        finalize_close_after_zero_process(&mut connection, FinalizeCloseCommand { request: close })
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    let epoch: String = connection
        .query_row("SELECT state FROM session_runtime_epochs", [], |r| r.get(0))
        .unwrap();
    let heads: i64 = connection
        .query_row("SELECT COUNT(*) FROM chat_session_heads", [], |r| r.get(0))
        .unwrap();
    assert_eq!((epoch.as_str(), heads), ("exited", 0));
}

#[test]
fn proven_zero_process_parks_without_closing_or_dropping_the_head() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-park")).unwrap();
    let created_page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(
        created_page.sessions.as_slice()[0].next_runtime_epoch,
        Some(RuntimeEpoch::new(1).unwrap())
    );
    let activation = ActivateSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        expected_projection_revision: ProjectionRevision::new(1).unwrap(),
        client_intent_id: ClientIntentId::parse("activate-park").unwrap(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        artifact_digest: "d".repeat(64),
        now_millis: 20,
    };
    activate_session(&mut connection, activation).unwrap();
    complete_activation(
        &mut connection,
        CompleteActivationCommand {
            binding_change: None,
            project_id: SageProjectId::parse("project-one").unwrap(),
            chat_id: SageChatId::parse("chat-one").unwrap(),
            session_id: started.session_id.clone(),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "d".repeat(64),
            external_session_id: ExternalId::new("external", "park-session").unwrap(),
            capabilities_json: capabilities(),
            now_millis: 25,
        },
    )
    .unwrap();
    let command = ParkSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        exit_kind: RequiredText::new("exitKind", "idleAfterTurn").unwrap(),
        now_millis: 30,
    };
    let parked = park_session_after_zero_process(&mut connection, command.clone()).unwrap();
    assert_eq!(
        (parked.state, parked.disposition),
        (SessionState::Ready, MutationDisposition::Applied)
    );
    assert_eq!(
        park_session_after_zero_process(&mut connection, command)
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    let row: (String, String, String, i64) = connection
        .query_row(
            "SELECT s.state,s.residency,e.state,
                    EXISTS(SELECT 1 FROM chat_session_heads h WHERE h.session_id=s.id)
             FROM agent_sessions s JOIN session_runtime_epochs e ON e.session_id=s.id
             WHERE s.id=?1",
            [started.session_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
        )
        .unwrap();
    assert_eq!(row, ("ready".into(), "cold".into(), "exited".into(), 1));
    let parked_page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    let projected = &parked_page.sessions.as_slice()[0];
    assert_eq!(projected.runtime_epoch, None);
    assert_eq!(
        projected.next_runtime_epoch,
        Some(RuntimeEpoch::new(2).unwrap())
    );
}

#[test]
fn stale_start_and_capacity_plus_one_preserve_existing_history() {
    let mut connection = setup();
    assert!(matches!(
        start_session(&mut connection, start(1, "stale")),
        Err(StoreError::Conflict {
            resource: "projectionRevision"
        })
    ));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    connection
        .execute(
            "UPDATE acp_project_usage SET session_count=?1",
            [crate::acp::limits::LIMITS.durable.sessions_per_project as i64],
        )
        .unwrap();
    assert!(matches!(
        start_session(&mut connection, start(0, "full")),
        Err(StoreError::Capacity {
            resource: "projectSessions",
            ..
        })
    ));
    assert_eq!(
        connection
            .query_row("SELECT COUNT(*) FROM agent_sessions", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
}

fn insert_completed_turn(
    connection: &Connection,
    session: &SageAgentSessionId,
    id: &str,
    created: i64,
    payload: &str,
) {
    connection.execute("INSERT INTO turns VALUES(?1,'project-one','chat-one',?2,1,?3,?1,'completed','correlated',0,?4,?4,?4)",params![id,session.as_str(),created,created]).unwrap();
    let body =
        serde_json::json!({"kind":"text","payload":{"role":"user","text":payload}}).to_string();
    connection.execute("INSERT INTO items VALUES(?1,'project-one','chat-one',?2,?3,1,0,'primary',NULL,'text','completed',NULL,NULL,?4,?5,?6,0,?7,?7,?7)",params![format!("item-{id}"),session.as_str(),id,body,body.len() as i64,super::facts::hex_digest(body.as_bytes()),created]).unwrap();
}

#[test]
fn chat_page_is_cursor_bounded_and_rejects_corrupt_payload_before_ingress() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start")).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",[started.session_id.as_str()]).unwrap();
    for index in 0..21 {
        insert_completed_turn(
            &connection,
            &started.session_id,
            &format!("turn-{index:02}"),
            index,
            "hello",
        );
    }
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(page.turns.as_slice().len(), 20);
    assert!(page.next_cursor.is_some());
    let next = load_chat_page(
        &connection,
        page.project_id.clone(),
        page.chat_id.clone(),
        page.next_cursor,
    )
    .unwrap();
    assert_eq!(next.turns.as_slice().len(), 1);
    connection
        .execute(
            "UPDATE items SET payload_digest=?1 WHERE id='item-turn-20'",
            ["b".repeat(64)],
        )
        .unwrap();
    assert!(matches!(
        load_chat_page(&connection, page.project_id, page.chat_id, None),
        Err(StoreError::Corrupt("item payload integrity"))
    ));
    assert!(matches!(
        load_chat_page(
            &connection,
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            Some(PageCursor::new("cursor", "bad").unwrap())
        ),
        Err(StoreError::Conflict {
            resource: "pageCursor"
        })
    ));
}

#[test]
fn chat_page_streams_a_full_512_item_turn_as_two_bounded_fragments() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start")).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",[started.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-one','project-one','chat-one',?1,1,0,'turn-intent','completed','correlated',0,1,1,1)",[started.session_id.as_str()]).unwrap();
    let body =
        serde_json::json!({"kind":"text","payload":{"role":"user","text":"hello"}}).to_string();
    let digest = super::facts::hex_digest(body.as_bytes());
    for ordinal in 0..512 {
        connection.execute("INSERT INTO items VALUES(?1,'project-one','chat-one',?2,'turn-one',1,?3,'primary',NULL,'text','completed',NULL,NULL,?4,?5,?6,0,1,1,1)",params![format!("item-{ordinal}"),started.session_id.as_str(),ordinal,body,body.len() as i64,digest]).unwrap();
    }
    let first = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(first.turns.as_slice().len(), 1);
    assert_eq!(first.turns.as_slice()[0].items.as_slice().len(), 256);
    assert_eq!(first.turns.as_slice()[0].items.as_slice()[0].ordinal, 256);
    assert_eq!(first.turns.as_slice()[0].items.as_slice()[255].ordinal, 511);
    let second = load_chat_page(
        &connection,
        first.project_id.clone(),
        first.chat_id.clone(),
        Some(first.next_cursor.expect("same-turn continuation")),
    )
    .unwrap();
    assert_eq!(second.turns.as_slice().len(), 1);
    assert_eq!(second.turns.as_slice()[0].id, first.turns.as_slice()[0].id);
    assert_eq!(second.turns.as_slice()[0].items.as_slice().len(), 256);
    assert_eq!(second.turns.as_slice()[0].items.as_slice()[0].ordinal, 0);
    assert_eq!(
        second.turns.as_slice()[0].items.as_slice()[255].ordinal,
        255
    );
    assert!(second.next_cursor.is_none());
    let mut ids = first.turns.as_slice()[0]
        .items
        .as_slice()
        .iter()
        .chain(second.turns.as_slice()[0].items.as_slice())
        .map(|item| item.id.as_str())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 512);
}

#[test]
fn chat_page_rejects_a_persisted_resource_from_a_foreign_root() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-resource")).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",[started.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-resource','project-one','chat-one',?1,1,0,'turn-intent','completed','correlated',0,1,1,1)",[started.session_id.as_str()]).unwrap();
    let body = serde_json::to_string(&NormalizedPayload::file_summary_only(
        crate::acp::intent::FileChangeFacts {
            resource: crate::acp::intent::SafeResourceLocation::Resource {
                resource: crate::acp::intent::SafeResourceRef::new(
                    crate::acp::contract::SageRootId::parse("root-foreign").unwrap(),
                    "src/lib.rs",
                )
                .unwrap(),
            },
            summary: crate::acp::contract::SafeText::new("summary", "changed").unwrap(),
            additions: Some(1),
            deletions: None,
            operation: crate::acp::intent::FileChangeOperation::Updated,
        },
        false,
    ))
    .unwrap();
    let digest = super::facts::hex_digest(body.as_bytes());
    connection.execute("INSERT INTO items VALUES('item-resource','project-one','chat-one',?1,'turn-resource',1,0,'primary',NULL,'fileChangeDiff','completed',NULL,NULL,?2,?3,?4,0,1,1,1)",params![started.session_id.as_str(),body,body.len() as i64,digest]).unwrap();

    assert!(matches!(
        load_chat_page(
            &connection,
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            None,
        ),
        Err(StoreError::Corrupt("resource root membership"))
    ));
}

#[test]
fn chat_page_projects_legacy_and_current_command_tool_rows_and_rejects_foreign_tool_roots() {
    fn seed(connection: &mut Connection, intent: &str) -> SageAgentSessionId {
        let started = start_session(connection, start(0, intent)).unwrap();
        connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",[started.session_id.as_str()]).unwrap();
        connection.execute("INSERT INTO turns VALUES('turn-command','project-one','chat-one',?1,1,0,'turn-intent','completed','correlated',0,1,1,1)",[started.session_id.as_str()]).unwrap();
        started.session_id
    }
    fn insert(
        connection: &Connection,
        session: &SageAgentSessionId,
        id: &str,
        ordinal: i64,
        kind: &str,
        payload: &str,
    ) {
        let digest = super::facts::hex_digest(payload.as_bytes());
        connection.execute("INSERT INTO items VALUES(?1,'project-one','chat-one',?2,'turn-command',1,?3,'primary',NULL,?4,'completed','completed',NULL,?5,?6,?7,0,1,1,1)",params![id,session.as_str(),ordinal,kind,payload,payload.len() as i64,digest]).unwrap();
    }

    let mut connection = setup();
    let session = seed(&mut connection, "legacy-command");
    insert(
        &connection,
        &session,
        "legacy-call",
        0,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"command","label":"Command","inputSummary":null}}"#,
    );
    insert(
        &connection,
        &session,
        "legacy-result",
        1,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"command","label":"Command","inputSummary":null,"outputSummary":null}}"#,
    );
    insert(
        &connection,
        &session,
        "current-result",
        2,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"command","label":"Command","inputSummary":null,"outputSummary":"safe","locations":[{"kind":"resource","resource":{"rootId":"root-one","relativePath":"src/lib.rs"}},{"kind":"unavailable","reason":"missing","label":"Path does not exist"}]}}"#,
    );
    insert(
        &connection,
        &session,
        "file-call",
        3,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"file","label":"Read file","inputSummary":null,"locations":[{"kind":"resource","resource":{"rootId":"root-one","relativePath":"src/lib.rs"}},{"kind":"unavailable","reason":"missing","label":"Path does not exist"}]}}"#,
    );
    insert(
        &connection,
        &session,
        "diff-result",
        4,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"diff","label":"Apply diff","inputSummary":null,"outputSummary":null,"locations":[{"kind":"resource","resource":{"rootId":"root-one","relativePath":"src/main.rs"}}]}}"#,
    );
    insert(
        &connection,
        &session,
        "empty-file-call",
        5,
        "toolCall",
        r#"{"kind":"toolCall","payload":{"category":"file","label":"Read file","inputSummary":null}}"#,
    );
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert!(matches!(
        page.turns.as_slice()[0].items.as_slice()[0].content,
        crate::acp::projection::ItemContent::ToolCall {
            output_summary: None,
            ..
        }
    ));
    assert!(matches!(
        page.turns.as_slice()[0].items.as_slice()[1].content,
        crate::acp::projection::ItemContent::ToolCall {
            output_summary: None,
            ..
        }
    ));
    let items = page.turns.as_slice()[0].items.as_slice();
    assert!(matches!(
        &items[3].content,
        crate::acp::projection::ItemContent::ToolCall {
            presentation: crate::acp::projection::ToolPresentation::Read { resources, .. },
            ..
        } if resources.as_slice().len() == 2
    ));
    assert!(matches!(
        &items[4].content,
        crate::acp::projection::ItemContent::ToolCall {
            presentation: crate::acp::projection::ToolPresentation::Edit { resources, .. },
            ..
        } if matches!(
            resources.as_slice(),
            [crate::acp::intent::SafeResourceLocation::Resource { resource }]
                if resource.relative_path.as_str() == "src/main.rs"
        )
    ));
    assert!(matches!(
        &items[5].content,
        crate::acp::projection::ItemContent::ToolCall {
            presentation: crate::acp::projection::ToolPresentation::Read { resources, .. },
            ..
        } if resources.as_slice().is_empty()
    ));

    let mut foreign = setup();
    let session = seed(&mut foreign, "foreign-command");
    let body = r#"{"kind":"toolCall","payload":{"category":"command","label":"Command","inputSummary":null,"outputSummary":"safe","locations":[{"kind":"resource","resource":{"rootId":"root-foreign","relativePath":"src/lib.rs"}}]}}"#;
    insert(&foreign, &session, "foreign-call", 0, "toolCall", body);
    assert!(matches!(
        load_chat_page(
            &foreign,
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            None
        ),
        Err(StoreError::Corrupt("resource root membership"))
    ));

    let mut unsafe_label = setup();
    let session = seed(&mut unsafe_label, "unsafe-unavailable-label");
    let body = r#"{"kind":"toolCall","payload":{"category":"file","label":"File","inputSummary":null,"locations":[{"kind":"unavailable","reason":"foreign","label":"C:\\private\\secret.txt"}]}}"#;
    insert(&unsafe_label, &session, "unsafe-label", 0, "toolCall", body);
    assert!(matches!(
        load_chat_page(
            &unsafe_label,
            SageProjectId::parse("project-one").unwrap(),
            SageChatId::parse("chat-one").unwrap(),
            None
        ),
        Err(StoreError::Corrupt("unavailable resource label"))
    ));
}

#[test]
fn chat_page_loads_an_empty_session_without_project_roots() {
    let mut connection = setup();
    connection.execute("DELETE FROM project_roots", []).unwrap();
    let started = start_session(&mut connection, start(0, "start-empty-session")).unwrap();

    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    assert_eq!(page.current_session_id, Some(started.session_id.clone()));
    assert_eq!(page.sessions.as_slice().len(), 1);
    assert_eq!(page.sessions.as_slice()[0].id, started.session_id);
    assert!(page.turns.as_slice().is_empty());
    assert!(page.next_cursor.is_none());
}

#[test]
fn chat_page_loads_a_turn_with_no_transcript_items() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-empty-turn")).unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",
            [started.session_id.as_str()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns VALUES('turn-empty','project-one','chat-one',?1,1,0,'turn-intent','completed','correlated',0,1,1,1)",
            [started.session_id.as_str()],
        )
        .unwrap();

    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();

    assert_eq!(page.turns.as_slice().len(), 1);
    assert!(page.turns.as_slice()[0].items.as_slice().is_empty());
}

#[test]
fn chat_page_projects_more_than_the_live_permission_window_from_history() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-permission-history")).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'exited',NULL,0,1,'fixture')",[started.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-permissions','project-one','chat-one',?1,1,0,'turn-intent','completed','correlated',0,1,1,1)",[started.session_id.as_str()]).unwrap();
    let body = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        category: ToolCategory::Execute,
        summary: crate::acp::contract::SafeText::new("summary", "Run command").unwrap(),
        offered_option_kinds: None,
        reason: None,
    })
    .unwrap();
    let digest = super::facts::hex_digest(body.as_bytes());
    for ordinal in 0..17 {
        let external = format!("external-{ordinal}");
        connection.execute("INSERT INTO items VALUES(?1,'project-one','chat-one',?2,'turn-permissions',1,?3,'primary',NULL,'permissionRequest','completed','declined',?4,?5,?6,?7,0,1,1,1)",params![format!("item-permission-{ordinal}"),started.session_id.as_str(),ordinal,external,body,body.len() as i64,digest]).unwrap();
        connection.execute("INSERT INTO server_requests VALUES(?1,'project-one','chat-one',?2,'turn-permissions',1,?3,?4,'permission',1,?5,'resolvedDeclined',?6,0,1,1,1)",params![format!("request-{ordinal}"),started.session_id.as_str(),"a".repeat(64),external,digest,body]).unwrap();
    }
    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    let items = page.turns.as_slice()[0].items.as_slice();
    assert_eq!(items.len(), 17);
    assert!(items.iter().all(|item| matches!(
        item.content,
        crate::acp::projection::ItemContent::PermissionRequest {
            state: crate::acp::lifecycle::ServerRequestState::ResolvedDeclined,
            ..
        }
    )));
}

#[test]
fn chat_page_preserves_a_durable_failed_closed_permission_terminal() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start-failed-permission")).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs VALUES('project-one','chat-one',?1,1,'failed',NULL,0,2,'protocolFailure')",[started.session_id.as_str()]).unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-permission','project-one','chat-one',?1,1,0,'turn-intent','failed','correlated',0,1,2,2)",[started.session_id.as_str()]).unwrap();
    let body = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        category: ToolCategory::Execute,
        summary: crate::acp::contract::SafeText::new("summary", "Run command").unwrap(),
        offered_option_kinds: None,
        reason: None,
    })
    .unwrap();
    let digest = super::facts::hex_digest(body.as_bytes());
    connection.execute("INSERT INTO items VALUES('item-permission','project-one','chat-one',?1,'turn-permission',1,0,'primary',NULL,'permissionRequest','failed','failed','external',?2,?3,?4,1,1,2,2)",params![started.session_id.as_str(),body,body.len() as i64,digest]).unwrap();
    connection.execute("INSERT INTO server_requests VALUES('request-one','project-one','chat-one',?1,'turn-permission',1,?2,'external','permission',1,?3,'resolvedDeclined',?4,1,1,2,2)",params![started.session_id.as_str(),"a".repeat(64),digest,body]).unwrap();

    let page = load_chat_page(
        &connection,
        SageProjectId::parse("project-one").unwrap(),
        SageChatId::parse("chat-one").unwrap(),
        None,
    )
    .unwrap();
    let item = &page.turns.as_slice()[0].items.as_slice()[0];
    assert_eq!(item.lifecycle, crate::acp::lifecycle::ItemLifecycle::Failed);
    assert_eq!(
        item.tool_state,
        Some(crate::acp::lifecycle::ToolState::Failed)
    );
    assert!(matches!(
        item.content,
        crate::acp::projection::ItemContent::PermissionRequest {
            state: crate::acp::lifecycle::ServerRequestState::ResolvedDeclined,
            ..
        }
    ));
}

pub(super) fn permission(session: &SageAgentSessionId) -> PermissionIntent {
    let normalized_payload_json = serde_json::to_string(&NormalizedPayload::PermissionRequest {
        category: ToolCategory::Execute,
        summary: crate::acp::contract::SafeText::new("summary", "Run command").unwrap(),
        offered_option_kinds: None,
        reason: None,
    })
    .unwrap();
    let payload_digest = super::facts::hex_digest(normalized_payload_json.as_bytes());
    PermissionIntent {
        request_id: SageServerRequestId::parse("request-one").unwrap(),
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        session_id: session.clone(),
        turn_id: SageTurnId::parse("turn-live").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        binding_digest: "a".repeat(64),
        external_request_id: ExternalId::new("external", "external-one").unwrap(),
        kind: RequiredText::new("kind", "permission").unwrap(),
        payload_digest: "f".repeat(64),
        normalized_payload_digest: payload_digest,
        normalized_payload_json,
    }
}
struct Transport;
impl DeclineTransport for Transport {
    fn queue_decline(&mut self, _: &PermissionIntent) -> Result<(), PermissionError> {
        Ok(())
    }
}

#[test]
fn permission_receive_and_decline_are_durable_idempotent_and_write_ordered() {
    let mut connection = setup();
    let started = start_session(&mut connection, start(0, "start")).unwrap();
    let activate = ActivateSessionCommand {
        project_id: SageProjectId::parse("project-one").unwrap(),
        chat_id: SageChatId::parse("chat-one").unwrap(),
        expected_projection_revision: ProjectionRevision::new(1).unwrap(),
        client_intent_id: ClientIntentId::parse("activate").unwrap(),
        session_id: started.session_id.clone(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
        artifact_digest: "d".repeat(64),
        now_millis: 2,
    };
    activate_session(&mut connection, activate).unwrap();
    connection
        .execute("UPDATE session_runtime_epochs SET state='running'", [])
        .unwrap();
    connection
        .execute("UPDATE agent_sessions SET state='active'", [])
        .unwrap();
    connection.execute("INSERT INTO turns VALUES('turn-live','project-one','chat-one',?1,1,0,'turn-intent','waitingApproval','correlated',0,3,3,NULL)",[started.session_id.as_str()]).unwrap();
    let intent = permission(&started.session_id);
    assert_eq!(
        receive_permission_request(&mut connection, &intent, 4)
            .unwrap()
            .disposition,
        MutationDisposition::Applied
    );
    let persisted: (i64, String, String) = connection
        .query_row(
            "SELECT r.payload_digest_version,r.payload_digest,i.payload_digest
             FROM server_requests r JOIN items i
               ON i.turn_id=r.turn_id AND i.external_item_id=r.external_request_id
             WHERE r.id=?1",
            [intent.request_id.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .unwrap();
    assert_eq!(persisted.0, 1);
    assert_eq!(persisted.1, intent.payload_digest);
    assert_eq!(persisted.2, intent.normalized_payload_digest);
    assert_ne!(persisted.1, persisted.2);
    assert_eq!(
        receive_permission_request(&mut connection, &intent, 4)
            .unwrap()
            .disposition,
        MutationDisposition::Duplicate
    );
    let mut changed = intent.clone();
    changed.normalized_payload_json =
        serde_json::to_string(&NormalizedPayload::PermissionRequest {
            category: ToolCategory::Execute,
            summary: crate::acp::contract::SafeText::new("summary", "Different command").unwrap(),
            offered_option_kinds: None,
            reason: None,
        })
        .unwrap();
    changed.normalized_payload_digest =
        super::facts::hex_digest(changed.normalized_payload_json.as_bytes());
    assert!(matches!(
        receive_permission_request(&mut connection, &changed, 4),
        Err(StoreError::Conflict {
            resource: "requestIdentity"
        })
    ));
    {
        let mut journal = DenyOnlyPermissionJournal::new(&mut connection, 5);
        assert_eq!(
            decline_permission(&mut journal, &mut Transport, &intent, 0, 5).unwrap(),
            PermissionDisposition::ResponseQueued
        );
        assert_eq!(
            decline_written(&mut journal, &intent).unwrap(),
            PermissionDisposition::ResolvedDeclined
        );
    }
    let state: String = connection
        .query_row("SELECT state FROM server_requests", [], |r| r.get(0))
        .unwrap();
    assert_eq!(state, "resolvedDeclined");
    let item: (String, String) = connection
        .query_row(
            "SELECT lifecycle,tool_state FROM items WHERE kind='permissionRequest'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    assert_eq!(item, ("completed".into(), "declined".into()));
}
