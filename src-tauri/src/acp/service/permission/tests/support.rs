use std::{
    sync::{Arc, Mutex},
    time::Instant,
};

use crate::acp::{
    bindings::adapter::AdapterCommand,
    contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
    runtime::{
        HostCancelIntent, HostError, HostPromptIntent, LiveRuntimeEngine, LiveRuntimePort,
        LiveWrite, RuntimeHost, permissions::PermissionDecision,
    },
    scheduler::RuntimeOwner,
    store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
};

use super::super::AcpService;

struct Port {
    owner: RuntimeOwner,
    database: crate::persistence::Database,
    commands: Arc<Mutex<Vec<AdapterCommand>>>,
}

impl LiveRuntimePort for Port {
    type ZeroProof = ();

    fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    fn resolve_permission(
        &mut self,
        owner: &RuntimeOwner,
        request_id: &SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<(), HostError> {
        if owner != &self.owner {
            return Err(HostError::Intent);
        }
        let state: String = self
            .database
            .connection()
            .query_row(
                "SELECT state FROM server_requests WHERE id=?1",
                [request_id.as_str()],
                |row| row.get(0),
            )
            .map_err(|_| HostError::Intent)?;
        let expected = match decision {
            PermissionDecision::Ask => "asked",
            PermissionDecision::AllowOnce
            | PermissionDecision::AllowAlways
            | PermissionDecision::Answer(_) => "allowPending",
            PermissionDecision::Deny => "declinePending",
        };
        if state != expected {
            return Err(HostError::State);
        }
        self.commands
            .lock()
            .unwrap()
            .push(AdapterCommand::ResolvePermission {
                request_id: request_id.clone(),
                decision,
            });
        Ok(())
    }

    fn prepare_prompt(
        &mut self,
        _: &HostPromptIntent,
    ) -> Result<crate::acp::contract::SageTurnId, HostError> {
        Err(HostError::State)
    }

    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
    ) -> Result<LiveWrite, HostError> {
        Err(HostError::State)
    }

    fn observe_send(
        &mut self,
        _: &RuntimeOwner,
        _: &crate::acp::contract::SageTurnId,
        _: SendObservation,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn begin_close(&mut self, _: &CloseSessionCommand) -> Result<(), HostError> {
        Err(HostError::State)
    }

    fn wait_zero(&mut self, _: &RuntimeOwner) -> Result<Self::ZeroProof, HostError> {
        Ok(())
    }

    fn finalize_close(&mut self, _: &CloseSessionCommand, _: ()) -> Result<(), (HostError, ())> {
        Ok(())
    }

    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

pub(crate) fn service_with_pending_requests(
    commands: Arc<Mutex<Vec<AdapterCommand>>>,
) -> (AcpService, RuntimeOwner) {
    service_with_pending_requests_and_deny_id(commands, "request-deny")
}

pub(crate) fn service_with_pending_requests_and_deny_id(
    commands: Arc<Mutex<Vec<AdapterCommand>>>,
    deny_id: &str,
) -> (AcpService, RuntimeOwner) {
    let database = crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
    let project = database
        .create_project_from_root(&std::env::current_dir().unwrap())
        .unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let owner = RuntimeOwner {
        project_id: crate::acp::contract::SageProjectId::parse(project.id).unwrap(),
        chat_id: crate::acp::contract::SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap(),
        session_id: SageAgentSessionId::parse("session-permission").unwrap(),
        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
    };
    let service = AcpService::new(database.clone()).unwrap();
    let digest = "a".repeat(64);
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO agent_sessions(
               id,project_id,chat_id,ordinal,binding_id,binding_digest,harness_version,model_id,
               external_session_id,capabilities_json,state,residency,legacy,revision,
               projection_revision,created_at,updated_at,terminal_at
             ) VALUES(?1,?2,?3,1,'binding',?4,NULL,NULL,NULL,'{}','active','hot',0,0,0,1,1,NULL)",
            rusqlite::params![
                owner.session_id.as_str(),
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                &digest,
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO session_runtime_epochs(
               project_id,chat_id,session_id,runtime_epoch,state,artifact_digest,started_at,
               terminal_at,exit_kind
             ) VALUES(?1,?2,?3,1,'running',NULL,1,NULL,NULL)",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT INTO turns(
               id,project_id,chat_id,session_id,runtime_epoch,ordinal,client_intent_id,state,
               delivery_state,revision,created_at,updated_at,terminal_at
             ) VALUES('turn-permission',?1,?2,?3,1,0,'intent-permission','running',
                      'completeBytes',0,1,1,NULL)",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
            ],
        )
        .unwrap();
    drop(connection);
    for (request_id, external_request_id) in [
        (deny_id, "request-deny"),
        ("request-allow", "request-allow"),
        ("request-ask", "request-ask"),
    ] {
        let offered = if external_request_id == "request-allow" {
            vec![crate::acp::intent::PermissionOptionKind::AllowOnce]
        } else {
            vec![crate::acp::intent::PermissionOptionKind::RejectOnce]
        };
        insert_permission_request(
            &database,
            &owner,
            request_id,
            external_request_id,
            &crate::acp::intent::NormalizedPayload::PermissionRequest {
                user_input: None,
                category: crate::acp::intent::ToolCategory::Other,
                summary: crate::acp::contract::SafeText::new("summary", "Permission").unwrap(),
                offered_option_kinds: Some(
                    crate::acp::contract::BoundedVec::new("offered", offered).unwrap(),
                ),
                reason: None,
            },
        );
    }
    let connection = database.connection();
    connection
        .execute(
            "INSERT INTO acp_session_usage(
               project_id,chat_id,session_id,turn_count,item_count,item_bytes,fact_count,
               fact_bytes,payload_bytes,revision
             ) VALUES(?1,?2,?3,0,0,0,0,0,0,0)",
            rusqlite::params![
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
            ],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO acp_project_usage(
               project_id,chat_count,session_count,turn_count,item_count,fact_count,
               payload_bytes,revision
             ) VALUES(?1,0,0,0,0,0,0,0)",
            [owner.project_id.as_str()],
        )
        .unwrap();
    connection
        .execute(
            "INSERT OR IGNORE INTO acp_store_usage(
               singleton,chat_count,session_count,turn_count,item_count,item_bytes,fact_count,
               fact_bytes,shared_payload_bytes,revision
             ) VALUES(1,0,0,0,0,0,0,0,0,0)",
            [],
        )
        .unwrap();
    drop(connection);
    let host = Arc::new(
        RuntimeHost::start(LiveRuntimeEngine::new(Port {
            owner: owner.clone(),
            database,
            commands,
        }))
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    (service, owner)
}

/// Records a permission request exactly as the store admits a runtime's
/// `PermissionRequest` event, in the seeded `turn-permission` Turn.
#[cfg(feature = "acp-fixture")]
pub(crate) fn admit_permission_request(
    service: &AcpService,
    owner: &RuntimeOwner,
    request_id: &str,
    payload: &crate::acp::intent::NormalizedPayload,
) {
    insert_permission_request(&service.database, owner, request_id, request_id, payload);
}

fn insert_permission_request(
    database: &crate::persistence::Database,
    owner: &RuntimeOwner,
    request_id: &str,
    external_request_id: &str,
    payload: &crate::acp::intent::NormalizedPayload,
) {
    let digest = "a".repeat(64);
    database
        .connection()
        .execute(
            "INSERT INTO server_requests(
               id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
               external_request_id,kind,payload_digest_version,payload_digest,state,
               normalized_payload_json,revision,created_at,updated_at,resolved_at
             ) VALUES(?1,?2,?3,?4,'turn-permission',1,?5,?7,'permission',1,?5,
                      'received',?6,0,1,1,NULL)",
            rusqlite::params![
                request_id,
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                &digest,
                serde_json::to_string(payload).unwrap(),
                external_request_id,
            ],
        )
        .unwrap();
}

pub(super) fn permission_snapshot(service: &AcpService) -> (String, i64, i64, i64) {
    permission_snapshot_for(service, "request-deny")
}

pub(super) fn permission_snapshot_for(
    service: &AcpService,
    request_id: &str,
) -> (String, i64, i64, i64) {
    let connection = service.database.connection();
    let (state, revision) = connection
        .query_row(
            "SELECT state,revision FROM server_requests WHERE id=?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let projection_revision = connection
        .query_row(
            "SELECT projection_revision FROM agent_sessions WHERE id='session-permission'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    let fact_count = connection
        .query_row("SELECT COUNT(*) FROM session_facts", [], |row| row.get(0))
        .unwrap();
    (state, revision, projection_revision, fact_count)
}
