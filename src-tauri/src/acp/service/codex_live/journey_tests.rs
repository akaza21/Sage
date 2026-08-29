use std::{
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
    time::{Duration, Instant},
};

use super::*;
use crate::{
    acp::{
        bindings::codex::{
            CodexAdmissionUnavailable, CodexBindingRegistration, CodexProjectRootAuthority,
        },
        contract::{
            ClientIntentId, ProjectGeneration, ProjectionRevision, RuntimeEpoch, SageChatId,
            SageProjectId, SageRootId, SageTurnId,
        },
        intent::{BindingId, Prompt},
        lifecycle::{SessionState, TurnState},
        runtime::{
            HostCancelIntent, ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher,
        },
        scheduler::RuntimeOwner,
        store::{
            ActivateSessionCommand, CloseSessionCommand, PrepareTurnCommand, StartSessionCommand,
        },
    },
    domain::ProjectLayoutRequest,
    project_registry::ProjectRootAuthority,
};

#[cfg(windows)]
static JOURNEY_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
#[cfg(windows)]
fn unavailable_binding_cannot_start_a_live_codex_session() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        let setup = FixtureSetup::in_memory(CodexBindingRegistration::unavailable_for_test()).await;

        assert!(matches!(
            setup
                .service
                .start_codex_session(&setup.root(), setup.start_command())
                .await,
            Err(CodexSessionError::Binding(_))
        ));
        assert!(matches!(
            setup
                .service
                .send_codex_turn(
                    setup.turn_command(&RuntimeOwner {
                        project_id: setup.project_id.clone(),
                        chat_id: setup.chat_id.clone(),
                        session_id: crate::acp::contract::SageAgentSessionId::parse(
                            "session-unavailable"
                        )
                        .unwrap(),
                        runtime_epoch: RuntimeEpoch::new(1).unwrap(),
                    })
                ),
            Err(CodexSessionError::Intent)
        ));
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_sequential_turns_share_one_session_and_converge_to_zero() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;

        let first = setup.send_and_wait_completed(&owner).await;
        let second = setup.send_and_wait_completed(&owner).await;
        assert_ne!(first, second);
        assert_completed_turns(&setup, &owner, &[first, second]);

        setup.close(&owner);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn two_chats_are_sequential_on_the_one_runtime_slot() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-two-turn-success",
                "characterization-two-turn-success",
            ]))
            .await;
        let owner_a = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let chat_b = setup.create_chat();
        let owner_b = setup
            .start_session_for_chat(chat_b, RuntimeEpoch::new(1).unwrap())
            .await;

        assert!(matches!(
            setup.activate(&owner_b).await,
            Err(CodexSessionError::Capacity)
        ));
        let turns_a = [
            setup.send_and_wait_completed(&owner_a).await,
            setup.send_and_wait_completed(&owner_a).await,
        ];
        setup.close(&owner_a);
        assert_job_zero();

        setup.activate(&owner_b).await.unwrap();
        setup.wait_for_ready(&owner_b);
        let turns_b = [
            setup.send_and_wait_completed(&owner_b).await,
            setup.send_and_wait_completed(&owner_b).await,
        ];
        assert_completed_turns(&setup, &owner_b, &turns_b);
        assert_completed_turns(&setup, &owner_a, &turns_a);

        setup.close(&owner_b);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap();
    });
}

#[test]
#[cfg(windows)]
fn restart_resume_then_another_turn_reads_sqlite() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        let registration = CodexBindingRegistration::admitted_for_journey_test(&[
            "codex-journey-turn-success",
            "characterization-resume-turn-success",
        ]);
        let setup = FixtureSetup::file_backed(registration.clone()).await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;
        let first = setup.send_and_wait_completed(&owner).await;
        let database_path = setup.database_path.clone().unwrap();
        let project_id = setup.project_id.clone();
        let chat_id = setup.chat_id.clone();
        let root_path = setup.root_path.clone();
        let root_id = setup.root_id.clone();

        let shutdown_started = Instant::now();
        setup.service.shutdown_blocking().unwrap_or_else(|error| {
            panic!(
                "restart first-life shutdown failed: {error:?} after {} in {}ms",
                setup.service.last_shutdown_stage(),
                shutdown_started.elapsed().as_millis()
            )
        });
        drop(setup);
        assert_job_zero();

        let database = crate::persistence::Database::open(&database_path).unwrap();
        let resumed = AcpService::new_with_codex_publisher_for_test(
            database,
            registration,
            std::sync::Arc::new(JourneyPublisher),
        )
        .unwrap();
        let page = resumed
            .load_chat_page(project_id.clone(), chat_id.clone(), None)
            .unwrap();
        assert!(
            page.sessions
                .as_slice()
                .iter()
                .any(|session| session.id == owner.session_id)
        );
        let resumed_owner = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner.clone()
        };
        let resumed = FixtureSetup::reopened(
            resumed,
            database_path.clone(),
            project_id,
            chat_id,
            root_path,
            root_id,
        );

        resumed.activate(&resumed_owner).await.unwrap();
        resumed.wait_for_ready(&resumed_owner);
        let second = resumed.send_and_wait_completed(&resumed_owner).await;
        assert_completed_turns(&resumed, &resumed_owner, &[first, second]);
        resumed.close(&resumed_owner);
        assert_job_zero();
        resumed.service.shutdown_blocking().unwrap();
        drop(resumed);
        let _ = std::fs::remove_dir_all(database_path.parent().unwrap());
    });
}

#[test]
#[cfg(windows)]
fn cancel_then_reuse_session_converges_to_zero_orphans() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        let setup =
            FixtureSetup::in_memory(CodexBindingRegistration::admitted_for_journey_test(&[
                "characterization-cancel-reuse",
                "characterization-resume-turn-success",
            ]))
            .await;
        let owner = setup
            .start_and_activate(RuntimeEpoch::new(1).unwrap())
            .await;

        setup
            .service
            .send_codex_turn(setup.turn_command(&owner))
            .unwrap();
        let turn_id = setup.wait_for_running_turn(&owner);
        setup
            .service
            .cancel_codex_turn(HostCancelIntent {
                owner: owner.clone(),
                turn_id: turn_id.clone(),
                client_intent_id: ClientIntentId::new(),
                now_millis: crate::persistence::now_millis(),
            })
            .unwrap_or_else(|error| {
                panic!(
                    "cancel seam failed: {error:?} after {}; turns={:?}",
                    setup.service.last_cancel_stage(),
                    setup
                        .page(&owner)
                        .turns
                        .as_slice()
                        .iter()
                        .map(|turn| (turn.id.as_str().to_owned(), format!("{:?}", turn.state)))
                        .collect::<Vec<_>>()
                )
            });
        setup.wait_for_cancelled(&owner);

        // The production cancel path must relinquish its Job before this
        // session is activated again. A direct slot mutation would not prove it.
        setup.wait_for_job_zero();
        let reuse_owner = RuntimeOwner {
            runtime_epoch: RuntimeEpoch::new(2).unwrap(),
            ..owner.clone()
        };
        setup.activate(&reuse_owner).await.unwrap();
        setup.wait_for_ready(&reuse_owner);
        setup.send_and_wait_completed(&reuse_owner).await;
        setup.close(&reuse_owner);
        assert_job_zero();
        setup.service.shutdown_blocking().unwrap_or_else(|error| {
            panic!(
                "cancel final shutdown failed: {error:?} after {}",
                setup.service.last_shutdown_stage()
            )
        });
    });
}

#[test]
#[cfg(windows)]
fn unsigned_or_observation_only_cannot_launch() {
    let _journey = journey_test_lock();
    tauri::async_runtime::block_on(async {
        for (registration, unavailable) in [
            (
                CodexBindingRegistration::unsigned_for_test(),
                CodexAdmissionUnavailable::IndependentAdmission,
            ),
            (
                CodexBindingRegistration::observation_only_for_test(),
                CodexAdmissionUnavailable::ObservationOnly,
            ),
        ] {
            let setup = FixtureSetup::in_memory(registration).await;
            assert!(matches!(
                setup.service.start_codex_session(&setup.root(), setup.start_command()).await,
                Err(CodexSessionError::Binding(reason)) if reason == unavailable
            ));
            assert_job_zero();
            setup.service.shutdown_blocking().unwrap();
        }
    });
}

#[cfg(windows)]
struct FixtureSetup {
    service: AcpService,
    project_id: SageProjectId,
    chat_id: SageChatId,
    root_id: SageRootId,
    root_path: PathBuf,
    database_path: Option<PathBuf>,
}

#[cfg(windows)]
impl FixtureSetup {
    async fn in_memory(registration: CodexBindingRegistration) -> Self {
        Self::open(Path::new(":memory:"), None, registration).await
    }

    async fn file_backed(registration: CodexBindingRegistration) -> Self {
        let directory =
            std::env::temp_dir().join(format!("sage-codex-journey-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&directory).unwrap();
        let path = directory.join("sage.db");
        Self::open(&path, Some(path.clone()), registration).await
    }

    async fn open(
        path: &Path,
        database_path: Option<PathBuf>,
        registration: CodexBindingRegistration,
    ) -> Self {
        let database = crate::persistence::Database::open(path).unwrap();
        let root_path = std::env::current_dir().unwrap().canonicalize().unwrap();
        let project = database.create_project_from_root(&root_path).unwrap();
        let snapshot = database.bootstrap_snapshot().unwrap();
        Self {
            service: AcpService::new_with_codex_publisher_for_test(
                database,
                registration,
                std::sync::Arc::new(JourneyPublisher),
            )
            .unwrap(),
            project_id: SageProjectId::parse(project.id).unwrap(),
            chat_id: SageChatId::parse(snapshot.active_tab_id.unwrap()).unwrap(),
            root_id: SageRootId::parse(project.roots[0].id.clone()).unwrap(),
            root_path,
            database_path,
        }
    }

    fn reopened(
        service: AcpService,
        database_path: PathBuf,
        project_id: SageProjectId,
        chat_id: SageChatId,
        root_path: PathBuf,
        root_id: SageRootId,
    ) -> Self {
        Self {
            service,
            project_id,
            chat_id,
            root_id,
            root_path,
            database_path: Some(database_path),
        }
    }

    fn root(&self) -> CodexProjectRootAuthority {
        CodexProjectRootAuthority::for_test(
            ProjectRootAuthority::for_test(
                self.project_id.clone(),
                ProjectGeneration::ZERO,
                self.root_id.clone(),
                &self.root_path,
            )
            .unwrap(),
        )
    }

    async fn start_and_activate(&self, epoch: RuntimeEpoch) -> RuntimeOwner {
        let owner = self
            .start_session_for_chat(self.chat_id.clone(), epoch)
            .await;
        self.activate(&owner).await.unwrap();
        self.wait_for_ready(&owner);
        owner
    }

    async fn start_session_for_chat(
        &self,
        chat_id: SageChatId,
        epoch: RuntimeEpoch,
    ) -> RuntimeOwner {
        crate::acp::service::tests::seed_codex_discovery(&self.service, &self.root(), None);
        let revision = self
            .service
            .load_chat_page(self.project_id.clone(), chat_id.clone(), None)
            .unwrap()
            .projection_revision;
        let started = self
            .service
            .start_codex_session(&self.root(), self.start_command_for(&chat_id, revision))
            .await
            .unwrap();
        RuntimeOwner {
            project_id: self.project_id.clone(),
            chat_id,
            session_id: started.session_id,
            runtime_epoch: epoch,
        }
    }

    async fn activate(&self, owner: &RuntimeOwner) -> Result<(), CodexSessionError> {
        self.service
            .activate_codex_session(self.activate_command(owner), self.root())
            .await
    }

    async fn send_and_wait_completed(&self, owner: &RuntimeOwner) -> SageTurnId {
        let completed_before = self.completed_turns(owner);
        self.service
            .send_codex_turn(self.turn_command(owner))
            .unwrap();
        self.wait_for_completed_after(owner, &completed_before)
    }

    fn close(&self, owner: &RuntimeOwner) {
        self.service
            .close_codex_session(self.close_command(owner))
            .unwrap();
    }

    fn create_chat(&self) -> SageChatId {
        let snapshot = self.service.database.bootstrap_snapshot().unwrap();
        SageChatId::parse(
            self.service
                .database
                .create_chat(&ProjectLayoutRequest {
                    project_id: self.project_id.as_str().to_owned(),
                    tabs: snapshot.tabs,
                    active_tab_id: snapshot.active_tab_id,
                    selected_root_id: snapshot.selected_root_id,
                    selected_resource: snapshot.selected_resource,
                })
                .unwrap(),
        )
        .unwrap()
    }

    fn start_command(&self) -> StartSessionCommand {
        self.start_command_for(&self.chat_id, ProjectionRevision::ZERO)
    }

    fn start_command_for(
        &self,
        chat_id: &SageChatId,
        revision: ProjectionRevision,
    ) -> StartSessionCommand {
        StartSessionCommand {
            project_id: self.project_id.clone(),
            chat_id: chat_id.clone(),
            expected_projection_revision: revision,
            client_intent_id: ClientIntentId::new(),
            binding_id: BindingId::new("bindingId", "codex-app-server").unwrap(),
            binding_digest: "forged".into(),
            harness_version: None,
            model_id: None,
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn activate_command(&self, owner: &RuntimeOwner) -> ActivateSessionCommand {
        ActivateSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            artifact_digest: "forged".into(),
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn turn_command(&self, owner: &RuntimeOwner) -> PrepareTurnCommand {
        PrepareTurnCommand {
            project_id: owner.project_id.clone(),
            project_generation: ProjectGeneration::ZERO,
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            prompt: Prompt::new("prompt", "Complete this short test turn.").unwrap(),
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn close_command(&self, owner: &RuntimeOwner) -> CloseSessionCommand {
        CloseSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: self.page(owner).projection_revision,
            client_intent_id: ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: owner.runtime_epoch,
            now_millis: crate::persistence::now_millis(),
        }
    }

    fn page(&self, owner: &RuntimeOwner) -> crate::acp::projection::AcpChatPage {
        self.service
            .load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .unwrap()
    }

    fn wait_for_ready(&self, owner: &RuntimeOwner) {
        self.wait_until(|| {
            self.page(owner).sessions.as_slice().iter().any(|session| {
                session.id == owner.session_id && session.state == SessionState::Ready
            })
        });
    }

    fn wait_for_completed_after(
        &self,
        owner: &RuntimeOwner,
        completed_before: &[SageTurnId],
    ) -> SageTurnId {
        self.wait_until(|| {
            self.completed_turns(owner)
                .iter()
                .any(|turn_id| !completed_before.contains(turn_id))
        });
        self.completed_turns(owner)
            .into_iter()
            .find(|turn_id| !completed_before.contains(turn_id))
            .unwrap()
    }

    fn wait_for_running_turn(&self, owner: &RuntimeOwner) -> SageTurnId {
        self.wait_until(|| {
            self.page(owner)
                .turns
                .as_slice()
                .iter()
                .any(|turn| turn.session_id == owner.session_id && turn.state == TurnState::Running)
        });
        self.page(owner)
            .turns
            .as_slice()
            .iter()
            .find(|turn| turn.session_id == owner.session_id && turn.state == TurnState::Running)
            .unwrap()
            .id
            .clone()
    }

    fn wait_for_cancelled(&self, owner: &RuntimeOwner) {
        self.wait_until(|| {
            self.page(owner).turns.as_slice().iter().any(|turn| {
                turn.session_id == owner.session_id
                    && matches!(turn.state, TurnState::Cancelled | TurnState::Interrupted)
            })
        });
    }

    fn completed_turns(&self, owner: &RuntimeOwner) -> Vec<SageTurnId> {
        self.page(owner)
            .turns
            .as_slice()
            .iter()
            .filter(|turn| {
                turn.session_id == owner.session_id && turn.state == TurnState::Completed
            })
            .map(|turn| turn.id.clone())
            .collect()
    }

    fn wait_for_job_zero(&self) {
        self.wait_until(|| {
            crate::acp::process::TreeCapacityPool::global()
                .observation_usage()
                .0
                == 0
        });
    }

    fn wait_until(&self, condition: impl Fn() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !condition() {
            if Instant::now() >= deadline {
                panic!("production Codex journey did not converge before its deadline");
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }
}

#[cfg(windows)]
struct JourneyPublisher;

#[cfg(windows)]
impl ProjectionPublisher for JourneyPublisher {
    fn publish_committed(
        &self,
        event: crate::acp::intent::ProjectionChangedV1,
    ) -> Result<ProjectionPublishStatus, ProjectionPublishError> {
        if event.projection_revision == ProjectionRevision::ZERO {
            return Err(ProjectionPublishError::InvalidRevision);
        }
        Ok(ProjectionPublishStatus::Published)
    }
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
fn assert_completed_turns(setup: &FixtureSetup, owner: &RuntimeOwner, expected: &[SageTurnId]) {
    let completed = setup.completed_turns(owner);
    assert_eq!(completed.len(), expected.len());
    for turn_id in expected {
        assert!(completed.contains(turn_id));
    }
}

#[cfg(windows)]
pub(crate) fn journey_test_lock() -> MutexGuard<'static, ()> {
    JOURNEY_TEST_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}
