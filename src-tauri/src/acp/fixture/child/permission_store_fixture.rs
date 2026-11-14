use rusqlite::OptionalExtension;

use crate::{
    acp::{
        contract::{
            RequiredText, RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId,
            SageServerRequestId, SageTurnId,
        },
        intent::ExternalId,
        lifecycle::ServerRequestState,
        recovery::{RecoveryReport, recover_after_restart},
        store::{
            AcpStore, DatabaseAcpStore, DenyOnlyPermissionJournal, RequestStateCommand,
            StoreMutation,
        },
    },
    acp_protocol::permissions::{
        DeclineTransport, PermissionDisposition, PermissionError, PermissionIntent,
        decline_permission,
    },
    persistence::Database,
};

pub(in crate::acp::fixture) const BINDING_DIGEST: [u8; 32] = [7; 32];

pub(in crate::acp::fixture) struct PermissionStoreFixture {
    database: Database,
}

impl PermissionStoreFixture {
    pub(in crate::acp::fixture) fn new() -> Self {
        let database = Database::open(std::path::Path::new(":memory:")).unwrap();
        seed(&database);
        Self { database }
    }

    pub(in crate::acp::fixture) fn database_store(&self) -> DatabaseAcpStore {
        DatabaseAcpStore::new(self.database.clone())
    }

    pub(in crate::acp::fixture) fn project_id(&self) -> SageProjectId {
        SageProjectId::parse("project-permission").unwrap()
    }

    pub(in crate::acp::fixture) fn chat_id(&self) -> SageChatId {
        SageChatId::parse("chat-permission").unwrap()
    }

    pub(in crate::acp::fixture) fn session_id(&self) -> SageAgentSessionId {
        SageAgentSessionId::parse("session-permission").unwrap()
    }

    pub(in crate::acp::fixture) fn turn_id(&self) -> SageTurnId {
        SageTurnId::parse("turn-permission").unwrap()
    }

    pub(in crate::acp::fixture) fn runtime_epoch(&self) -> RuntimeEpoch {
        RuntimeEpoch::new(1).unwrap()
    }

    pub(in crate::acp::fixture) fn recover(&self, now_millis: i64) -> RecoveryReport {
        recover_after_restart(&mut self.database.connection(), now_millis).unwrap()
    }

    pub(in crate::acp::fixture) fn session_state(&self) -> String {
        self.text("SELECT state FROM agent_sessions WHERE id='session-permission'")
    }

    pub(super) fn request_state(&self) -> Option<String> {
        self.database
            .connection()
            .query_row(
                "SELECT state FROM server_requests WHERE session_id='session-permission'",
                [],
                |row| row.get(0),
            )
            .optional()
            .unwrap()
    }

    pub(super) fn request_count(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM server_requests WHERE session_id='session-permission'")
    }

    pub(super) fn decline_fact_count(&self) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM session_facts
             WHERE session_id='session-permission' AND kind='requestState'
               AND payload_json LIKE '%declineCommitted%'",
        )
    }

    pub(super) fn runtime_epoch_state(&self) -> String {
        self.text(
            "SELECT state FROM session_runtime_epochs
             WHERE session_id='session-permission' AND runtime_epoch=1",
        )
    }

    pub(super) fn live_head_count(&self) -> i64 {
        self.count(
            "SELECT COUNT(*) FROM chat_session_heads
             WHERE session_id='session-permission'",
        )
    }

    pub(super) fn record_deny_decision(&self, now_millis: i64) -> SageServerRequestId {
        let request_id = SageServerRequestId::parse(self.text(
            "SELECT id FROM server_requests
             WHERE session_id='session-permission' AND external_request_id='permission-a'",
        ))
        .unwrap();
        let mut store = self.database_store();
        store
            .commit(&[StoreMutation::RequestState(RequestStateCommand {
                project_id: self.project_id(),
                chat_id: self.chat_id(),
                session_id: self.session_id(),
                turn_id: self.turn_id(),
                runtime_epoch: self.runtime_epoch(),
                request_id: request_id.clone(),
                state: ServerRequestState::DeclinePending,
                now_millis,
            })])
            .unwrap();
        request_id
    }

    #[cfg(windows)]
    pub(in crate::acp::fixture) fn session_count(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM agent_sessions WHERE chat_id='chat-permission'")
    }

    #[cfg(windows)]
    pub(in crate::acp::fixture) fn activity_count(&self) -> i64 {
        self.count("SELECT COUNT(*) FROM session_activities WHERE turn_id='turn-permission'")
    }

    #[cfg(windows)]
    pub(in crate::acp::fixture) fn has_projected_activity_text(
        &self,
        external_activity: &str,
        parent_external_item: &str,
        expected_text: &str,
    ) -> bool {
        let connection = self.database.connection();
        let page =
            crate::acp::store::load_chat_page(&connection, self.project_id(), self.chat_id(), None)
                .unwrap();
        let projected = page.turns.as_slice().iter().any(|turn| {
            turn.items.as_slice().iter().any(|item| {
                item.origin == crate::acp::projection::ItemOrigin::HarnessActivity
                    && item.activity_id.is_some()
                    && matches!(
                        &item.content,
                        crate::acp::projection::ItemContent::Text { text, .. }
                            if text.as_str() == expected_text
                    )
            })
        });
        let linked: bool = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM items child
                 JOIN session_activities activity ON activity.id=child.activity_id
                 JOIN items parent ON parent.id=activity.parent_item_id
                 WHERE child.turn_id='turn-permission' AND child.origin='harnessActivity'
                   AND activity.external_activity_id=?1 AND parent.external_item_id=?2)",
                [external_activity, parent_external_item],
                |row| row.get(0),
            )
            .unwrap();
        projected && linked
    }

    pub(super) fn probe_decline_replay(&self, now_millis: i64) -> ReplayProbe {
        let connection = self.database.connection();
        let row: (String, String, String, String, String, String, String) = connection
            .query_row(
                "SELECT r.id,r.binding_digest,r.external_request_id,r.kind,r.payload_digest,
                        r.normalized_payload_json,i.payload_digest
                 FROM server_requests r JOIN items i
                   ON i.session_id=r.session_id
                  AND i.external_item_id=r.external_request_id
                 WHERE r.session_id='session-permission'",
                [],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                        row.get(5)?,
                        row.get(6)?,
                    ))
                },
            )
            .unwrap();
        drop(connection);
        let intent = PermissionIntent {
            request_id: SageServerRequestId::parse(&row.0).unwrap(),
            project_id: self.project_id(),
            chat_id: self.chat_id(),
            session_id: self.session_id(),
            turn_id: self.turn_id(),
            runtime_epoch: self.runtime_epoch(),
            binding_digest: row.1,
            external_request_id: ExternalId::new("fixturePermissionId", &row.2).unwrap(),
            kind: RequiredText::new("permissionKind", &row.3).unwrap(),
            payload_digest: row.4,
            normalized_payload_json: row.5,
            normalized_payload_digest: row.6,
        };
        let mut connection = self.database.connection();
        let mut journal = DenyOnlyPermissionJournal::new(&mut connection, now_millis);
        let mut transport = ReplayTransport::default();
        let disposition = decline_permission(
            &mut journal,
            &mut transport,
            &intent,
            0,
            u64::try_from(now_millis).unwrap(),
        )
        .unwrap();
        ReplayProbe {
            duplicate: disposition == PermissionDisposition::Duplicate,
            writes: transport.writes,
        }
    }

    fn text(&self, sql: &str) -> String {
        self.database
            .connection()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }

    fn count(&self, sql: &str) -> i64 {
        self.database
            .connection()
            .query_row(sql, [], |row| row.get(0))
            .unwrap()
    }
}

pub(super) struct ReplayProbe {
    pub(super) duplicate: bool,
    pub(super) writes: usize,
}

#[derive(Default)]
struct ReplayTransport {
    writes: usize,
}

impl DeclineTransport for ReplayTransport {
    fn queue_decline(&mut self, _intent: &PermissionIntent) -> Result<(), PermissionError> {
        self.writes = self.writes.checked_add(1).expect("bounded replay probe");
        Ok(())
    }
}

fn seed(database: &Database) {
    let connection = database.connection();
    connection.execute("INSERT INTO projects(id,name,created_at,updated_at,last_opened_at) VALUES('project-permission','Permission fixture',1,1,1)",[]).unwrap();
    connection.execute("INSERT INTO project_roots(id,project_id,absolute_path,canonical_key,ordinal) VALUES('root-permission','project-permission','C:/fixture','c:/fixture',0)",[]).unwrap();
    connection.execute("INSERT INTO chats(id,project_id,ordinal,title,projection_revision,created_at,updated_at) VALUES('chat-permission','project-permission',0,'Permission fixture',0,1,1)",[]).unwrap();
    connection.execute("INSERT INTO agent_sessions(id,project_id,chat_id,ordinal,binding_id,binding_digest,external_session_id,capabilities_json,state,residency,legacy,revision,projection_revision,created_at,updated_at) VALUES('session-permission','project-permission','chat-permission',0,'fixture',?1,'fixture-session',?2,'active','hot',0,0,0,1,1)",rusqlite::params![hex(&BINDING_DIGEST), pending_capabilities()]).unwrap();
    connection.execute("INSERT INTO chat_session_heads(project_id,chat_id,session_id,installed_revision,installed_at) VALUES('project-permission','chat-permission','session-permission',0,1)",[]).unwrap();
    connection.execute("INSERT INTO session_runtime_epochs(project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at) VALUES('project-permission','chat-permission','session-permission',1,'running',?1,1)",["d".repeat(64)]).unwrap();
    connection.execute("INSERT INTO turns(id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,delivery_state,revision,created_at,updated_at) VALUES('turn-permission','project-permission','chat-permission','session-permission',1,0,'message-permission','running','correlated',0,1,1)",[]).unwrap();
    connection
        .execute(
            "INSERT INTO acp_project_usage VALUES('project-permission',1,1,1,0,0,0,0)",
            [],
        )
        .unwrap();
    connection.execute("INSERT INTO acp_session_usage VALUES('project-permission','chat-permission','session-permission',1,0,0,0,0,0,0)",[]).unwrap();
    connection.execute("UPDATE acp_store_usage SET chat_count=1,session_count=1,turn_count=1 WHERE singleton=1",[]).unwrap();
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn pending_capabilities() -> &'static str {
    r#"{"schemaVersion":1,"protocolVersion":"pending","newSession":false,"resumeSession":false,"loadSession":false,"closeSession":false,"prompt":false,"cancel":false,"nativeSessionFork":false,"activeTurnSteer":false,"childConversationProjection":false,"childTranscriptRead":false,"collaborationEventProjection":false,"multiRootExtensionSchemaId":null,"modelCount":0,"configCount":0}"#
}
