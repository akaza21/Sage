use std::{collections::HashMap, path::Path, sync::Arc, time::Instant};

use super::*;
use crate::{
    acp::{
        bindings::codex::{CodexBindingRegistration, CodexProjectRootAuthority},
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SafeText,
            SageAgentSessionId, SageChatId, SageProjectId, SageRootId, SageTurnId,
        },
        intent::{BindingId, ExternalId},
        process::{StdinEndpoint, StdoutEndpoint, TreeControl, TreeExit},
        runtime::{
            HostCancelIntent, HostCloseIntent, HostError, HostPromptIntent, LiveRunnerHandle,
            PromptWrite, RuntimeEngine, RuntimeHost,
        },
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, BeginCancelCommand, CloseSessionCommand,
            CompleteActivationCommand, FinalizeCloseCommand, PrepareTurnCommand,
            TurnMutationCommand,
        },
    },
    domain::ProjectLayoutRequest,
    project_registry::ProjectRootAuthority,
};

#[test]
#[cfg(windows)]
fn unavailable_binding_cannot_start_a_live_codex_session() {
    tauri::async_runtime::block_on(async {
        let setup = fixture_setup(CodexBindingRegistration::unavailable_for_test()).await;
        assert!(matches!(
            setup
                .service
                .start_codex_session(
                    &setup.root,
                    start_command(&setup.project_id, &setup.chat_id, ProjectionRevision::ZERO),
                )
                .await,
            Err(CodexSessionError::Binding(_))
        ));
        assert!(matches!(
            setup.service.send_codex_turn(turn_command(
                &setup.service,
                &RuntimeOwner {
                    project_id: setup.project_id.clone(),
                    chat_id: setup.chat_id.clone(),
                    session_id: SageAgentSessionId::parse("session-unavailable").unwrap(),
                    runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                }
            )),
            Err(CodexSessionError::Intent)
        ));
        assert!(setup.service.runtime.active().is_none());
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_sequential_turns_share_one_session_and_converge_to_zero() {
    tauri::async_runtime::block_on(async {
        let setup = fixture_setup(CodexBindingRegistration::admitted_for_test()).await;
        let owner =
            start_admitted_session(&setup, "two-turns", RuntimeEpoch::new(1).unwrap()).await;
        let control = attach_fixture_runtime(&setup.service, &owner);
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner))
                .is_ok()
        );
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner))
                .is_ok()
        );
        assert_eq!(durable_turn_ids(&setup.service, &owner).len(), 2);
        assert!(
            setup
                .service
                .close_codex_session(close_command(&setup.service, &owner))
                .is_ok()
        );
        assert_eq!(control.active_processes().unwrap(), 0);
        assert!(setup.service.runtime.active().is_none());
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_chats_are_sequential_on_the_one_runtime_slot() {
    tauri::async_runtime::block_on(async {
        let setup = fixture_setup(CodexBindingRegistration::admitted_for_test()).await;
        let owner_a = start_admitted_session(&setup, "chat-a", RuntimeEpoch::new(1).unwrap()).await;
        let control_a = attach_fixture_runtime(&setup.service, &owner_a);
        let chat_b = create_durable_chat(&setup.database, &setup.project_id);
        let owner_b = start_admitted_session_for_chat(
            &setup,
            chat_b,
            "chat-b",
            RuntimeEpoch::new(2).unwrap(),
        )
        .await;
        assert_eq!(
            setup.service.runtime.reserve_start(owner_b.clone()),
            Err(super::super::runtime_slot::RuntimeSlotError::Occupied),
        );
        assert!(
            setup
                .service
                .close_codex_session(close_command(&setup.service, &owner_a))
                .is_ok()
        );
        assert_eq!(control_a.active_processes().unwrap(), 0);
        assert!(setup.service.runtime.active().is_none());
        assert_job_zero();
        let control_b = attach_fixture_runtime(&setup.service, &owner_b);
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner_b))
                .is_ok()
        );
        assert!(
            setup
                .service
                .close_codex_session(close_command(&setup.service, &owner_b))
                .is_ok()
        );
        assert_eq!(control_b.active_processes().unwrap(), 0);
        assert_ne!(owner_a.chat_id, owner_b.chat_id);
        assert!(durable_turn_ids(&setup.service, &owner_a).is_empty());
        assert_eq!(durable_turn_ids(&setup.service, &owner_b).len(), 1);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn restart_resume_then_another_turn_reads_sqlite() {
    tauri::async_runtime::block_on(async {
        let setup = fixture_setup(CodexBindingRegistration::admitted_for_test()).await;
        let owner = start_admitted_session(&setup, "restart", RuntimeEpoch::new(1).unwrap()).await;
        let control = attach_fixture_runtime(&setup.service, &owner);
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner))
                .is_ok()
        );
        let FixtureSetup {
            database,
            service,
            project_id,
            chat_id,
            root,
        } = setup;
        drop(service);
        assert_eq!(control.active_processes().unwrap(), 0);
        assert_job_zero();
        drop(control);
        let resumed = AcpService::new_with_codex_for_test(
            database.clone(),
            CodexBindingRegistration::admitted_for_test(),
        )
        .unwrap();
        let page = resumed
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap();
        assert!(
            page.sessions
                .as_slice()
                .iter()
                .any(|session| session.id == owner.session_id)
        );
        let resumed_setup = FixtureSetup {
            database,
            service: resumed,
            project_id,
            chat_id,
            root,
        };
        let control = attach_fixture_runtime(&resumed_setup.service, &owner);
        assert!(
            resumed_setup
                .service
                .send_codex_turn(turn_command(&resumed_setup.service, &owner))
                .is_ok()
        );
        assert!(
            resumed_setup
                .service
                .close_codex_session(close_command(&resumed_setup.service, &owner))
                .is_ok()
        );
        assert_eq!(control.active_processes().unwrap(), 0);
        assert_job_zero();
        resumed_setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn cancel_then_reuse_session_converges_to_zero_orphans() {
    tauri::async_runtime::block_on(async {
        let setup = fixture_setup(CodexBindingRegistration::admitted_for_test()).await;
        let owner =
            start_admitted_session(&setup, "cancel-reuse", RuntimeEpoch::new(1).unwrap()).await;
        let control = attach_fixture_runtime(&setup.service, &owner);
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner))
                .is_ok()
        );
        let turn_id = durable_turn_ids(&setup.service, &owner).pop().unwrap();
        assert!(
            setup
                .service
                .cancel_codex_turn(HostCancelIntent {
                    owner: owner.clone(),
                    turn_id: SageTurnId::parse(turn_id).unwrap(),
                    client_intent_id: ClientIntentId::new(),
                    now_millis: crate::persistence::now_millis(),
                })
                .is_ok()
        );
        assert_eq!(control.active_processes().unwrap(), 0);
        assert_job_zero();
        assert!(
            setup
                .service
                .send_codex_turn(turn_command(&setup.service, &owner))
                .is_ok()
        );
        assert!(
            setup
                .service
                .close_codex_session(close_command(&setup.service, &owner))
                .is_ok()
        );
        assert_eq!(control.active_processes().unwrap(), 0);
        assert!(setup.service.runtime.active().is_none());
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn unsigned_or_observation_only_cannot_launch() {
    tauri::async_runtime::block_on(async {
        for registration in [
            CodexBindingRegistration::unsigned_for_test(),
            CodexBindingRegistration::observation_only_for_test(),
        ] {
            let setup = fixture_setup(registration).await;
            assert!(matches!(
                setup
                    .service
                    .start_codex_session(
                        &setup.root,
                        start_command(&setup.project_id, &setup.chat_id, ProjectionRevision::ZERO),
                    )
                    .await,
                Err(CodexSessionError::Binding(_))
            ));
            assert!(setup.service.runtime.active().is_none());
            assert_job_zero();
            setup.service.shutdown_blocking().unwrap();
        }
    });
}

#[cfg(windows)]
struct FixtureSetup {
    database: crate::persistence::Database,
    service: AcpService,
    project_id: SageProjectId,
    chat_id: SageChatId,
    root: CodexProjectRootAuthority,
}

#[cfg(windows)]
async fn fixture_setup(registration: CodexBindingRegistration) -> FixtureSetup {
    let database = crate::persistence::Database::open(Path::new(":memory:")).unwrap();
    let root_path = std::env::current_dir().unwrap().canonicalize().unwrap();
    let project = database.create_project_from_root(&root_path).unwrap();
    let snapshot = database.bootstrap_snapshot().unwrap();
    let project_id = SageProjectId::parse(project.id).unwrap();
    let chat_id = SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap();
    let root = codex_root(&project_id, &project.roots[0].id, &root_path);
    let service = AcpService::new_with_codex_for_test(database.clone(), registration).unwrap();
    FixtureSetup {
        database,
        service,
        project_id,
        chat_id,
        root,
    }
}

#[cfg(windows)]
async fn start_admitted_session(
    setup: &FixtureSetup,
    label: &str,
    epoch: RuntimeEpoch,
) -> RuntimeOwner {
    start_admitted_session_for_chat(setup, setup.chat_id.clone(), label, epoch).await
}

#[cfg(windows)]
async fn start_admitted_session_for_chat(
    setup: &FixtureSetup,
    chat_id: SageChatId,
    label: &str,
    epoch: RuntimeEpoch,
) -> RuntimeOwner {
    crate::acp::service::tests::seed_codex_discovery(&setup.service, &setup.root, None);
    let revision = setup
        .service
        .load_chat_page(setup.project_id.clone(), chat_id.clone(), None)
        .unwrap()
        .projection_revision;
    let started = setup
        .service
        .start_codex_session(
            &setup.root,
            start_command(&setup.project_id, &chat_id, revision),
        )
        .await
        .unwrap();
    let owner = RuntimeOwner {
        project_id: setup.project_id.clone(),
        chat_id: chat_id.clone(),
        session_id: started.session_id.clone(),
        runtime_epoch: epoch,
    };
    let binding = setup.service.codex_registration().refresh().unwrap();
    let mut lease = setup
        .service
        .sessions
        .activate_session(ActivateSessionCommand {
            project_id: setup.project_id.clone(),
            chat_id,
            expected_projection_revision: started.projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: started.session_id,
            runtime_epoch: epoch,
            artifact_digest: binding.artifact_digest().to_owned(),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    lease
        .complete_activation(CompleteActivationCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: epoch,
            binding_digest: binding.binding_digest_hex().to_owned(),
            artifact_digest: binding.artifact_digest().to_owned(),
            external_session_id: ExternalId::new("session", format!("fixture-{label}")).unwrap(),
            capabilities_json: serde_json::to_string(&binding.capability_ceiling().unwrap())
                .unwrap(),
            now_millis: crate::persistence::now_millis(),
        })
        .await
        .unwrap();
    drop(lease);
    owner
}

#[cfg(windows)]
fn attach_fixture_runtime(service: &AcpService, owner: &RuntimeOwner) -> TreeControl {
    let running = crate::acp::process::launch_fixture_for_test(owner.runtime_epoch.get());
    let (stdin, stdout, control) = running.into_parts();
    let runner = LiveRunnerHandle::new(
        Arc::new(crate::acp::runtime::DriverMailbox::new().unwrap()),
        control.clone(),
    );
    let host = Arc::new(
        RuntimeHost::start_with_shutdown_recovery(
            FixtureRuntime::new(
                owner.clone(),
                service.database.clone(),
                control.clone(),
                stdin,
                stdout,
            ),
            runner,
        )
        .unwrap(),
    );
    host.activate(owner.clone()).unwrap();
    service.install_runtime_for_test(owner.clone(), host);
    control
}

#[cfg(windows)]
fn durable_turn_ids(service: &AcpService, owner: &RuntimeOwner) -> Vec<String> {
    service
        .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
        .unwrap()
        .turns
        .as_slice()
        .iter()
        .filter(|turn| turn.session_id == owner.session_id)
        .map(|turn| turn.id.as_str().to_owned())
        .collect()
}

#[cfg(windows)]
fn turn_command(service: &AcpService, owner: &RuntimeOwner) -> PrepareTurnCommand {
    PrepareTurnCommand {
        project_id: owner.project_id.clone(),
        project_generation: ProjectGeneration::ZERO,
        chat_id: owner.chat_id.clone(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        expected_projection_revision: service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
            .projection_revision,
        client_intent_id: ClientIntentId::new(),
        prompt: crate::acp::intent::Prompt::new("prompt", "journey turn").unwrap(),
        now_millis: crate::persistence::now_millis(),
    }
}

#[cfg(windows)]
fn close_command(service: &AcpService, owner: &RuntimeOwner) -> CloseSessionCommand {
    CloseSessionCommand {
        project_id: owner.project_id.clone(),
        chat_id: owner.chat_id.clone(),
        expected_projection_revision: service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
            .projection_revision,
        client_intent_id: ClientIntentId::new(),
        session_id: owner.session_id.clone(),
        runtime_epoch: owner.runtime_epoch,
        now_millis: crate::persistence::now_millis(),
    }
}

#[cfg(windows)]
fn start_command(
    project_id: &SageProjectId,
    chat_id: &SageChatId,
    revision: ProjectionRevision,
) -> crate::acp::store::StartSessionCommand {
    crate::acp::store::StartSessionCommand {
        project_id: project_id.clone(),
        chat_id: chat_id.clone(),
        expected_projection_revision: revision,
        client_intent_id: ClientIntentId::new(),
        binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
        binding_digest: "forged".into(),
        harness_version: Some(SafeText::new("version", "forged").unwrap()),
        model_id: None,
        now_millis: crate::persistence::now_millis(),
    }
}

#[cfg(windows)]
fn create_durable_chat(
    database: &crate::persistence::Database,
    project_id: &SageProjectId,
) -> SageChatId {
    let snapshot = database.bootstrap_snapshot().unwrap();
    SageChatId::parse(
        database
            .create_chat(&ProjectLayoutRequest {
                project_id: project_id.as_str().to_owned(),
                tabs: snapshot.tabs,
                active_tab_id: snapshot.active_tab_id,
                selected_root_id: snapshot.selected_root_id,
                selected_resource: snapshot.selected_resource,
            })
            .unwrap(),
    )
    .unwrap()
}

#[cfg(windows)]
fn codex_root(project_id: &SageProjectId, root_id: &str, root: &Path) -> CodexProjectRootAuthority {
    CodexProjectRootAuthority::for_test(
        ProjectRootAuthority::for_test(
            project_id.clone(),
            ProjectGeneration::ZERO,
            SageRootId::parse(root_id).unwrap(),
            root,
        )
        .unwrap(),
    )
}

#[cfg(windows)]
fn assert_job_zero() {
    assert_eq!(
        crate::acp::process::TreeCapacityPool::global()
            .observation_usage()
            .0,
        0
    );
}

#[cfg(windows)]
struct FixtureRuntime {
    owner: RuntimeOwner,
    database: crate::persistence::Database,
    control: TreeControl,
    turns: HashMap<SageTurnId, TurnMutationCommand>,
    _stdin: StdinEndpoint,
    _stdout: StdoutEndpoint,
}

#[cfg(windows)]
impl FixtureRuntime {
    fn new(
        owner: RuntimeOwner,
        database: crate::persistence::Database,
        control: TreeControl,
        stdin: StdinEndpoint,
        stdout: StdoutEndpoint,
    ) -> Self {
        Self {
            owner,
            database,
            control,
            turns: HashMap::new(),
            _stdin: stdin,
            _stdout: stdout,
        }
    }

    fn mutation(&self, turn_id: &SageTurnId) -> Result<TurnMutationCommand, HostError> {
        self.turns.get(turn_id).cloned().ok_or(HostError::Intent)
    }

    fn update(&mut self, turn_id: &SageTurnId, revision: crate::acp::contract::EntityRevision) {
        self.turns.get_mut(turn_id).unwrap().expected_revision = revision;
    }

    fn zero(&self) -> Result<(), HostError> {
        self.control
            .request_termination(TreeExit::Cancelled)
            .map_err(|_| HostError::RecoveryRequired)?;
        self.control
            .wait_zero_within(std::time::Duration::from_secs(3))
            .map(|_| ())
            .map_err(|_| HostError::RecoveryRequired)
    }
}

#[cfg(windows)]
impl RuntimeEngine for FixtureRuntime {
    fn activate(&mut self, owner: &RuntimeOwner) -> Result<(), HostError> {
        (owner == &self.owner)
            .then_some(())
            .ok_or(HostError::Intent)
    }
    fn prepare_prompt(&mut self, intent: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        if intent.owner() != self.owner {
            return Err(HostError::Intent);
        }
        let prepared = self
            .database
            .acp_prepare_turn(intent.command.clone())
            .map_err(|_| HostError::Intent)?;
        self.turns.insert(
            prepared.turn_id.clone(),
            TurnMutationCommand {
                project_id: self.owner.project_id.clone(),
                chat_id: self.owner.chat_id.clone(),
                session_id: self.owner.session_id.clone(),
                runtime_epoch: self.owner.runtime_epoch,
                turn_id: prepared.turn_id.clone(),
                expected_revision: prepared.turn_revision,
                now_millis: crate::persistence::now_millis(),
            },
        );
        Ok(prepared.turn_id)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        let sent = self
            .database
            .acp_begin_send(self.mutation(turn_id)?)
            .map_err(|_| HostError::Intent)?;
        self.update(turn_id, sent.revision);
        let observed = self
            .database
            .acp_record_send_observation(
                self.mutation(turn_id)?,
                crate::acp::store::SendObservation::CompleteBytes,
            )
            .map_err(|_| HostError::Intent)?;
        self.update(turn_id, observed.revision);
        Ok(PromptWrite::Complete(turn_id.clone()))
    }
    fn retry_prompt(
        &mut self,
        owner: &RuntimeOwner,
        turn_id: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        self.dispatch_prompt(owner, turn_id)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Err(HostError::RecoveryRequired)
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Err(HostError::RecoveryRequired)
    }
    fn begin_cancel(&mut self, intent: &HostCancelIntent) -> Result<(), HostError> {
        let mut mutation = self
            .database
            .write_connection()
            .map_err(|_| HostError::RecoveryRequired)?;
        let command = self.mutation(&intent.turn_id)?;
        let cancelled = crate::acp::store::begin_cancel(
            &mut mutation,
            BeginCancelCommand {
                turn: command,
                client_intent_id: intent.client_intent_id.clone(),
            },
        )
        .map_err(|_| HostError::Intent)?;
        drop(mutation);
        self.update(&intent.turn_id, cancelled.revision);
        Ok(())
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        self.zero()
    }
    fn accept_terminal(
        &mut self,
        _: &crate::acp::store::AdapterTurnCursor,
    ) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        let mut connection = self
            .database
            .write_connection()
            .map_err(|_| HostError::RecoveryRequired)?;
        crate::acp::store::close_session(&mut connection, intent.command.clone())
            .map_err(|_| HostError::Intent)?;
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        self.zero()
    }
    fn finalize_close(&mut self, intent: &HostCloseIntent) -> Result<(), HostError> {
        let mut connection = self
            .database
            .write_connection()
            .map_err(|_| HostError::RecoveryRequired)?;
        crate::acp::store::finalize_close_after_zero_process(
            &mut connection,
            FinalizeCloseCommand {
                request: intent.command.clone(),
            },
        )
        .map_err(|_| HostError::Intent)?;
        Ok(())
    }
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        self.zero()
    }
}
