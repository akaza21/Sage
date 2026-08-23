use crate::acp::{
    contract::{
        RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId, SageServerRequestId,
        SageTurnId,
    },
    lifecycle::ServerRequestState,
    runtime::{HostError, JournalLane, permissions::PermissionDecision},
    scheduler::RuntimeOwner,
    store::{RequestStateCommand, StoreMutation},
};

use super::AcpService;

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct PermissionDecisionResponse {
    project_id: SageProjectId,
    chat_id: SageChatId,
    session_id: SageAgentSessionId,
    request_id: SageServerRequestId,
    decision: PermissionDecision,
    state: &'static str,
}

impl AcpService {
    pub(crate) fn resolve_permission_decision(
        &self,
        project_id: SageProjectId,
        chat_id: SageChatId,
        session_id: SageAgentSessionId,
        request_id: SageServerRequestId,
        decision: PermissionDecision,
    ) -> Result<PermissionDecisionResponse, HostError> {
        let (epoch, turn_id): (i64, String) = self
            .database
            .connection()
            .query_row(
                "SELECT runtime_epoch,turn_id FROM server_requests
                 WHERE id=?1 AND project_id=?2 AND chat_id=?3 AND session_id=?4 LIMIT 1",
                rusqlite::params![
                    request_id.as_str(),
                    project_id.as_str(),
                    chat_id.as_str(),
                    session_id.as_str()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| HostError::Intent)?;
        let runtime_epoch = RuntimeEpoch::new(u64::try_from(epoch).map_err(|_| HostError::Intent)?)
            .map_err(|_| HostError::Intent)?;
        let turn_id = SageTurnId::parse(turn_id).map_err(|_| HostError::Intent)?;
        let state = match decision {
            PermissionDecision::Ask => ServerRequestState::Asked,
            PermissionDecision::AllowOnce => ServerRequestState::AllowPending,
            PermissionDecision::Deny => ServerRequestState::DeclinePending,
        };
        self.journal
            .enqueue_one(
                JournalLane::Control,
                StoreMutation::RequestState(RequestStateCommand {
                    project_id: project_id.clone(),
                    chat_id: chat_id.clone(),
                    session_id: session_id.clone(),
                    turn_id,
                    runtime_epoch,
                    request_id: request_id.clone(),
                    state,
                    now_millis: crate::persistence::now_millis(),
                }),
                u64::try_from(crate::persistence::now_millis())
                    .unwrap_or_default()
                    .saturating_add(crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc),
            )
            .and_then(|ticket| ticket.blocking_committed())
            .map_err(|_| HostError::RecoveryRequired)?;
        let owner = RuntimeOwner {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: session_id.clone(),
            runtime_epoch,
        };
        self.runtime
            .for_owner(&owner)
            .map_err(|_| HostError::Intent)?
            .resolve_permission(owner, request_id.clone(), decision)?;
        Ok(PermissionDecisionResponse {
            project_id,
            chat_id,
            session_id,
            request_id,
            decision,
            state: match state {
                ServerRequestState::Asked => "asked",
                ServerRequestState::AllowPending => "allowPending",
                ServerRequestState::DeclinePending => "declinePending",
                _ => unreachable!("permission decision target is fixed"),
            },
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Mutex},
        time::Instant,
    };

    use crate::acp::{
        bindings::adapter::AdapterCommand,
        contract::{RuntimeEpoch, SageAgentSessionId, SageServerRequestId},
        runtime::{
            HostCancelIntent, HostError, HostPromptIntent, LiveRuntimeEngine, LiveRuntimePort,
            LiveWrite, RuntimeHost,
        },
        scheduler::RuntimeOwner,
        store::{AdapterTurnCursor, CloseSessionCommand, SendObservation},
    };

    use super::{AcpService, PermissionDecision};

    struct Port {
        owner: RuntimeOwner,
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

        fn finalize_close(
            &mut self,
            _: &CloseSessionCommand,
            _: (),
        ) -> Result<(), (HostError, ())> {
            Ok(())
        }

        fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
            Ok(())
        }
    }

    fn service_with_pending_requests(
        commands: Arc<Mutex<Vec<AdapterCommand>>>,
    ) -> (AcpService, RuntimeOwner) {
        let database =
            crate::persistence::Database::open(std::path::Path::new(":memory:")).unwrap();
        let project = database
            .create_project_from_root(&std::env::current_dir().unwrap())
            .unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        let owner = RuntimeOwner {
            project_id: crate::acp::contract::SageProjectId::parse(project.id).unwrap(),
            chat_id: crate::acp::contract::SageChatId::parse(snapshot.active_tab_id.unwrap())
                .unwrap(),
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
        for request_id in ["request-deny", "request-allow", "request-ask"] {
            connection
                .execute(
                    "INSERT INTO server_requests(
                   id,project_id,chat_id,session_id,turn_id,runtime_epoch,binding_digest,
                   external_request_id,kind,payload_digest_version,payload_digest,state,
                   normalized_payload_json,revision,created_at,updated_at,resolved_at
                 ) VALUES(?1,?2,?3,?4,'turn-permission',1,?5,?1,'permission',1,?5,
                          'received','{}',0,1,1,NULL)",
                    rusqlite::params![
                        request_id,
                        owner.project_id.as_str(),
                        owner.chat_id.as_str(),
                        owner.session_id.as_str(),
                        &digest,
                    ],
                )
                .unwrap();
        }
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
                commands,
            }))
            .unwrap(),
        );
        host.activate(owner.clone()).unwrap();
        service.install_runtime_for_test(owner.clone(), host);
        (service, owner)
    }

    #[test]
    fn resolve_permission_decision_reaches_live_runtime_port() {
        let commands = Arc::new(Mutex::new(Vec::new()));
        let (service, owner) = service_with_pending_requests(Arc::clone(&commands));
        for (request_id, decision) in [
            ("request-deny", PermissionDecision::Deny),
            ("request-allow", PermissionDecision::AllowOnce),
            ("request-ask", PermissionDecision::Ask),
        ] {
            service
                .resolve_permission_decision(
                    owner.project_id.clone(),
                    owner.chat_id.clone(),
                    owner.session_id.clone(),
                    SageServerRequestId::parse(request_id).unwrap(),
                    decision,
                )
                .unwrap();
        }
        let decisions = commands
            .lock()
            .unwrap()
            .drain(..)
            .map(|command| match command {
                AdapterCommand::ResolvePermission {
                    request_id,
                    decision,
                } => (request_id, decision),
                _ => panic!("permission routing must use AdapterCommand::ResolvePermission"),
            })
            .collect::<Vec<_>>();
        assert_eq!(
            decisions
                .iter()
                .map(|(_, decision)| *decision)
                .collect::<Vec<_>>(),
            vec![
                PermissionDecision::Deny,
                PermissionDecision::AllowOnce,
                PermissionDecision::Ask,
            ]
        );
        service.shutdown_blocking().unwrap();
    }
}
