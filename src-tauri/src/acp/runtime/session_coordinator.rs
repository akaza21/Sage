use std::sync::Arc;

use crate::acp::{
    contract::{RuntimeEpoch, SageAgentSessionId, SageChatId, SageProjectId},
    intent::ProjectionChangedV1,
    process::ZeroProcessToken,
    scheduler::{RuntimeOwner, Scheduler, SchedulerError, TreeLease},
    store::{
        ActivateSessionCommand, CloseSessionCommand, CompleteActivationCommand,
        FailActivationCommand, FinalizeCloseCommand, ParkSessionCommand, SessionMutation,
        StartSessionCommand, StoreCommit, StoreError, StoreMutation, StoreMutationResult,
    },
};

use super::{
    coordinator::{PrepareDispatch, ProjectIntentAuthority, SendCoordinator},
    journal::{JournalError, JournalHandle, JournalLane, now_millis},
    projection_publisher::{ProjectionPublishError, ProjectionPublishStatus, ProjectionPublisher},
};

mod lease;

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionCoordinatorError {
    #[error("the session/runtime owner is stale or foreign")]
    Intent,
    #[error("the session tree has already converged to zero processes")]
    Converged,
    #[error(transparent)]
    Scheduler(#[from] SchedulerError),
    #[error(transparent)]
    Journal(#[from] JournalError),
    #[error(transparent)]
    Projection(#[from] ProjectionPublishError),
    #[error("journal returned an unexpected session mutation result")]
    JournalProtocol,
    #[error("projection mailbox changed owner; authoritative bootstrap is required")]
    BootstrapRequired,
}

#[derive(Clone)]
pub(crate) struct SessionCoordinator {
    journal: JournalHandle,
    scheduler: Scheduler,
    publisher: Arc<dyn ProjectionPublisher>,
}

impl std::fmt::Debug for SessionCoordinator {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SessionCoordinator")
            .field("journal", &self.journal)
            .field("scheduler", &self.scheduler)
            .finish_non_exhaustive()
    }
}

/// Native process supervision creates this only after checking the exact
/// runtime owner and observing no surviving process or descendant.
#[derive(Debug)]
pub(crate) struct ZeroProcessProof {
    owner: RuntimeOwner,
}

/// Owns the sole live adapter-tree reservation. Dropping this value never
/// advertises capacity; convergence is an explicit, zero-process operation.
#[derive(Debug)]
pub(crate) struct SessionTreeLease {
    coordinator: SessionCoordinator,
    owner: RuntimeOwner,
    tree: Option<TreeLease>,
}

pub(crate) struct ActivationFailure {
    pub(crate) error: SessionCoordinatorError,
    pub(crate) recovery: Option<ActivationRecovery>,
}

#[derive(Debug)]
pub(crate) struct ActivationRecovery {
    coordinator: SessionCoordinator,
    owner: RuntimeOwner,
    command: ActivateSessionCommand,
    tree: TreeLease,
    committed: Option<SessionMutation>,
}

impl std::fmt::Debug for ActivationFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ActivationFailure")
            .field("error", &self.error)
            .field("recovery_retained", &self.recovery.is_some())
            .finish()
    }
}

impl ActivationRecovery {
    pub(crate) fn owner(&self) -> &RuntimeOwner {
        &self.owner
    }

    pub(crate) fn command(&self) -> &ActivateSessionCommand {
        &self.command
    }

    pub(crate) fn committed(&self) -> Option<&SessionMutation> {
        self.committed.as_ref()
    }

    // Recovery owns the sole tree lease and activation command; returning it
    // intact prevents a false release while commit status is unresolved.
    #[allow(clippy::result_large_err)]
    pub(crate) fn into_committed_lease(self) -> Result<SessionTreeLease, Self> {
        if self.committed.is_none() {
            return Err(self);
        }
        Ok(SessionTreeLease {
            coordinator: self.coordinator,
            owner: self.owner,
            tree: Some(self.tree),
        })
    }

    #[cfg(test)]
    fn abort_proven_absent(self) -> Result<(), SessionCoordinatorError> {
        if self.committed.is_some() {
            return Err(SessionCoordinatorError::Intent);
        }
        self.tree.zero_process_converged()?;
        Ok(())
    }
}

impl SessionCoordinator {
    pub(crate) fn new(
        journal: JournalHandle,
        scheduler: Scheduler,
        publisher: Arc<dyn ProjectionPublisher>,
    ) -> Self {
        Self {
            journal,
            scheduler,
            publisher,
        }
    }

    pub(crate) async fn start_session(
        &self,
        command: StartSessionCommand,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        let project = command.project_id.clone();
        let chat = command.chat_id.clone();
        let mutation = self
            .commit(JournalLane::Data, StoreMutation::StartSession(command))
            .await?;
        self.publish(&project, &chat, &mutation)?;
        Ok(mutation)
    }

    /// Reserves the global tree before the durable activation intent. A known
    /// prelaunch rejection releases it; commit-unknown and post-commit errors
    /// deliberately leave capacity charged for authoritative recovery.
    pub(crate) async fn activate_session(
        &self,
        command: ActivateSessionCommand,
    ) -> Result<SessionTreeLease, Box<ActivationFailure>> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let tree = self
            .scheduler
            .reserve_tree(owner.clone())
            .map_err(|error| {
                Box::new(ActivationFailure {
                    error: error.into(),
                    recovery: None,
                })
            })?;
        let retained_command = command.clone();
        let ticket = match self.journal.enqueue_one(
            JournalLane::Data,
            StoreMutation::ActivateSession(command),
            deadline(JournalLane::Data),
        ) {
            Ok(ticket) => ticket,
            Err(error) => return release_known_prelaunch(tree, error),
        };
        let commit = match ticket.committed().await {
            Ok(commit) => commit,
            Err(error) if !commit_unknown(&error) => {
                return release_known_prelaunch(tree, error);
            }
            Err(error) => {
                return Err(Box::new(ActivationFailure {
                    error: error.into(),
                    recovery: Some(ActivationRecovery {
                        coordinator: self.clone(),
                        owner,
                        command: retained_command,
                        tree,
                        committed: None,
                    }),
                }));
            }
        };
        let mutation = match one_session(commit) {
            Ok(mutation) => mutation,
            Err(error) => {
                return Err(Box::new(ActivationFailure {
                    error,
                    recovery: Some(ActivationRecovery {
                        coordinator: self.clone(),
                        owner,
                        command: retained_command,
                        tree,
                        committed: None,
                    }),
                }));
            }
        };
        if let Err(error) = validate_mutation(&owner, &mutation) {
            return Err(Box::new(ActivationFailure {
                error,
                recovery: Some(ActivationRecovery {
                    coordinator: self.clone(),
                    owner,
                    command: retained_command,
                    tree,
                    committed: Some(mutation),
                }),
            }));
        }
        if let Err(error) = self.publish(&owner.project_id, &owner.chat_id, &mutation) {
            return Err(Box::new(ActivationFailure {
                error,
                recovery: Some(ActivationRecovery {
                    coordinator: self.clone(),
                    owner,
                    command: retained_command,
                    tree,
                    committed: Some(mutation),
                }),
            }));
        }
        Ok(SessionTreeLease {
            coordinator: self.clone(),
            owner,
            tree: Some(tree),
        })
    }

    async fn commit(
        &self,
        lane: JournalLane,
        mutation: StoreMutation,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        let commit = self
            .journal
            .enqueue_one(lane, mutation, deadline(lane))?
            .committed()
            .await?;
        one_session(commit)
    }

    fn publish(
        &self,
        project_id: &SageProjectId,
        chat_id: &SageChatId,
        mutation: &SessionMutation,
    ) -> Result<(), SessionCoordinatorError> {
        let disposition = self.publisher.publish_committed(ProjectionChangedV1 {
            project_id: project_id.clone(),
            chat_id: chat_id.clone(),
            session_id: mutation.session_id.clone(),
            projection_revision: mutation.projection_revision,
        })?;
        if matches!(
            disposition,
            ProjectionPublishStatus::BootstrapRequired | ProjectionPublishStatus::RecoveryRequired
        ) {
            Err(SessionCoordinatorError::BootstrapRequired)
        } else {
            Ok(())
        }
    }
}

fn one_session(commit: StoreCommit) -> Result<SessionMutation, SessionCoordinatorError> {
    match commit.results.as_slice() {
        [StoreMutationResult::Session(mutation)]
            if mutation.projection_revision == commit.projection_revision =>
        {
            Ok(mutation.clone())
        }
        _ => Err(SessionCoordinatorError::JournalProtocol),
    }
}

fn validate_mutation(
    owner: &RuntimeOwner,
    mutation: &SessionMutation,
) -> Result<(), SessionCoordinatorError> {
    if mutation.session_id != owner.session_id
        || mutation.runtime_epoch != Some(owner.runtime_epoch)
    {
        return Err(SessionCoordinatorError::JournalProtocol);
    }
    Ok(())
}

fn release_known_prelaunch(
    tree: TreeLease,
    error: JournalError,
) -> Result<SessionTreeLease, Box<ActivationFailure>> {
    match tree.zero_process_converged() {
        Ok(()) => Err(Box::new(ActivationFailure {
            error: error.into(),
            recovery: None,
        })),
        Err(scheduler) => Err(Box::new(ActivationFailure {
            error: scheduler.into(),
            recovery: None,
        })),
    }
}

fn commit_unknown(error: &JournalError) -> bool {
    matches!(error, JournalError::Store(StoreError::CommitUnknown))
}

fn deadline(lane: JournalLane) -> u64 {
    now_millis().saturating_add(match lane {
        JournalLane::Data => 30_000,
        JournalLane::Control => 5_000,
    })
}

#[cfg(test)]
mod tests {
    use std::{collections::VecDeque, sync::Mutex};

    use crate::acp::{
        contract::{
            ClientIntentId, EntityRevision, ProjectionRevision, RequiredText, RuntimeEpoch,
            SafeText,
        },
        intent::{BindingId, ExternalId, ModelId},
        lifecycle::SessionState,
        runtime::ProjectionMailbox,
        store::{AcpStore, MutationDisposition, StoreCommit},
    };

    use super::*;

    struct Step {
        kind: &'static str,
        result: Result<StoreCommit, StoreError>,
        mailbox_must_be_empty: bool,
    }

    struct FakeStore {
        steps: Arc<Mutex<VecDeque<Step>>>,
        seen: Arc<Mutex<Vec<&'static str>>>,
        mailbox: Arc<ProjectionMailbox>,
        scheduler: Scheduler,
    }

    impl AcpStore for FakeStore {
        fn commit(&mut self, batch: &[StoreMutation]) -> Result<StoreCommit, StoreError> {
            assert_eq!(batch.len(), 1);
            let kind = kind(&batch[0]);
            if kind == "activate" {
                assert!(
                    self.scheduler.active().is_some(),
                    "tree must be reserved before activation commit"
                );
            }
            let step = self.steps.lock().unwrap().pop_front().unwrap();
            assert_eq!(kind, step.kind);
            if step.mailbox_must_be_empty {
                assert!(self.mailbox.take().is_none(), "published before commit");
            }
            self.seen.lock().unwrap().push(kind);
            step.result
        }
    }

    struct Harness {
        coordinator: SessionCoordinator,
        scheduler: Scheduler,
        journal: JournalHandle,
        runner: tokio::task::JoinHandle<()>,
        mailbox: Arc<ProjectionMailbox>,
        seen: Arc<Mutex<Vec<&'static str>>>,
    }

    impl Harness {
        fn new(steps: Vec<Step>) -> Self {
            let scheduler = Scheduler::new();
            let mailbox = Arc::new(ProjectionMailbox::default());
            let seen = Arc::new(Mutex::new(Vec::new()));
            let (journal, runner) = JournalHandle::new(FakeStore {
                steps: Arc::new(Mutex::new(steps.into())),
                seen: Arc::clone(&seen),
                mailbox: Arc::clone(&mailbox),
                scheduler: scheduler.clone(),
            });
            let runner = runner.spawn();
            Self {
                coordinator: SessionCoordinator::new(
                    journal.clone(),
                    scheduler.clone(),
                    mailbox.clone(),
                ),
                scheduler,
                journal,
                runner,
                mailbox,
                seen,
            }
        }

        async fn finish(self) {
            self.journal.close_when_drained();
            self.runner.await.unwrap();
        }
    }

    #[test]
    fn start_commits_before_publish_and_accepts_duplicate() {
        runtime().block_on(async {
            let h = Harness::new(vec![
                ok(
                    "start",
                    session(1, SessionState::Created, None, false),
                    true,
                ),
                ok("start", session(1, SessionState::Created, None, true), true),
            ]);
            let first = h.coordinator.start_session(start()).await.unwrap();
            assert_eq!(first.disposition, MutationDisposition::Applied);
            assert_eq!(h.mailbox.take().unwrap().projection_revision.get(), 1);
            let duplicate = h.coordinator.start_session(start()).await.unwrap();
            assert_eq!(duplicate.disposition, MutationDisposition::Duplicate);
            assert_eq!(h.mailbox.take().unwrap().projection_revision.get(), 1);
            assert_eq!(*h.seen.lock().unwrap(), ["start", "start"]);
            h.finish().await;
        });
    }

    #[test]
    fn activation_reserves_the_only_tree_before_store() {
        runtime().block_on(async {
            let h = Harness::new(vec![ok(
                "activate",
                session(2, SessionState::Starting, Some(1), false),
                true,
            )]);
            let lease = h.coordinator.activate_session(activate()).await.unwrap();
            h.mailbox.take();
            assert!(matches!(
                h.coordinator.activate_session(activate_two()).await,
                Err(failure) if matches!(
                    failure.error,
                    SessionCoordinatorError::Scheduler(SchedulerError::TreeCapacity)
                )
            ));
            assert_eq!(h.seen.lock().unwrap().len(), 1);
            drop(lease);
            assert!(h.scheduler.active().is_some(), "Drop must fail closed");
            h.finish().await;
        });
    }

    #[test]
    fn known_prelaunch_failure_releases_but_commit_unknown_retains() {
        runtime().block_on(async {
            let known = Harness::new(vec![Step {
                kind: "activate",
                result: Err(StoreError::Conflict {
                    resource: "fixture",
                }),
                mailbox_must_be_empty: true,
            }]);
            assert!(
                known
                    .coordinator
                    .activate_session(activate())
                    .await
                    .is_err()
            );
            assert!(known.scheduler.active().is_none());
            known.finish().await;

            let unknown = Harness::new(vec![Step {
                kind: "activate",
                result: Err(StoreError::CommitUnknown),
                mailbox_must_be_empty: true,
            }]);
            let mut failure = unknown
                .coordinator
                .activate_session(activate())
                .await
                .unwrap_err();
            assert!(matches!(
                failure.error,
                SessionCoordinatorError::Journal(JournalError::Store(StoreError::CommitUnknown))
            ));
            let recovery = failure.recovery.take().expect("retained tree reservation");
            assert_eq!(recovery.owner(), &owner("one", 1));
            assert_eq!(recovery.command(), &activate());
            assert!(recovery.committed().is_none());
            assert!(unknown.scheduler.active().is_some());
            assert!(matches!(
                unknown.coordinator.activate_session(activate_two()).await,
                Err(failure) if matches!(
                    failure.error,
                    SessionCoordinatorError::Scheduler(SchedulerError::TreeCapacity)
                )
            ));
            recovery.abort_proven_absent().unwrap();
            assert!(unknown.scheduler.active().is_none());
            unknown.finish().await;
        });
    }

    #[test]
    fn stale_tuple_is_rejected_and_bootstrap_contention_stays_charged() {
        runtime().block_on(async {
            let h = Harness::new(vec![ok(
                "activate",
                session(2, SessionState::Starting, Some(1), false),
                false,
            )]);
            h.mailbox
                .publish_committed(ProjectionChangedV1 {
                    project_id: project("foreign"),
                    chat_id: chat("foreign"),
                    session_id: session_id("foreign"),
                    projection_revision: revision(1),
                })
                .unwrap();
            let mut failure = h
                .coordinator
                .activate_session(activate())
                .await
                .unwrap_err();
            assert!(matches!(
                failure.error,
                SessionCoordinatorError::BootstrapRequired
            ));
            let recovery = failure.recovery.take().expect("retained committed lease");
            assert_eq!(recovery.owner(), &owner("one", 1));
            assert!(recovery.committed().is_some());
            let lease = recovery.into_committed_lease().expect("known commit");
            assert_eq!(lease.owner(), &owner("one", 1));
            assert!(h.scheduler.active().is_some());
            assert!(matches!(
                h.coordinator.activate_session(activate_two()).await,
                Err(failure) if matches!(
                    failure.error,
                    SessionCoordinatorError::Scheduler(SchedulerError::TreeCapacity)
                )
            ));
            drop(lease);
            h.finish().await;

            let h = Harness::new(vec![ok(
                "activate",
                session(2, SessionState::Starting, Some(1), false),
                true,
            )]);
            let mut lease = h.coordinator.activate_session(activate()).await.unwrap();
            h.mailbox.take();
            let mut foreign = complete();
            foreign.session_id = session_id("foreign");
            assert!(matches!(
                lease.complete_activation(foreign).await,
                Err(SessionCoordinatorError::Intent)
            ));
            assert_eq!(h.seen.lock().unwrap().len(), 1);
            h.finish().await;
        });
    }

    #[test]
    fn close_and_park_converge_only_with_exact_zero_process_proof() {
        runtime().block_on(async {
            let h = Harness::new(vec![
                ok(
                    "activate",
                    session(2, SessionState::Starting, Some(1), false),
                    true,
                ),
                ok(
                    "complete",
                    session(3, SessionState::Ready, Some(1), false),
                    true,
                ),
                ok(
                    "close",
                    session(4, SessionState::Closing, Some(1), false),
                    true,
                ),
                ok(
                    "finalize",
                    session(5, SessionState::Closed, Some(1), false),
                    true,
                ),
            ]);
            let mut lease = h.coordinator.activate_session(activate()).await.unwrap();
            h.mailbox.take();
            lease.complete_activation(complete()).await.unwrap();
            h.mailbox.take();
            lease.close_session(close()).await.unwrap();
            h.mailbox.take();
            let foreign = ZeroProcessProof::confirmed(owner("foreign", 1));
            assert!(matches!(
                lease
                    .finalize_close_after_zero_process(finalize(), foreign)
                    .await,
                Err(SessionCoordinatorError::Intent)
            ));
            assert!(h.scheduler.active().is_some());
            lease
                .finalize_close_after_zero_process(
                    finalize(),
                    ZeroProcessProof::confirmed(owner("one", 1)),
                )
                .await
                .unwrap();
            assert!(h.scheduler.active().is_none());
            h.finish().await;

            let h = Harness::new(vec![
                ok(
                    "activate",
                    session(2, SessionState::Starting, Some(1), true),
                    true,
                ),
                ok(
                    "complete",
                    session(3, SessionState::Ready, Some(1), true),
                    true,
                ),
                ok("park", session(4, SessionState::Ready, Some(1), true), true),
            ]);
            let mut lease = h.coordinator.activate_session(activate()).await.unwrap();
            h.mailbox.take();
            lease.complete_activation(complete()).await.unwrap();
            h.mailbox.take();
            lease
                .park_after_zero_process(park(), ZeroProcessProof::confirmed(owner("one", 1)))
                .await
                .unwrap();
            assert!(h.scheduler.active().is_none());
            h.finish().await;
        });
    }

    #[test]
    fn duplicate_activation_failure_converges_after_zero_process_proof() {
        runtime().block_on(async {
            let h = Harness::new(vec![
                ok(
                    "activate",
                    session(2, SessionState::Starting, Some(1), false),
                    true,
                ),
                ok(
                    "fail",
                    session(3, SessionState::Failed, Some(1), true),
                    true,
                ),
            ]);
            let mut lease = h.coordinator.activate_session(activate()).await.unwrap();
            h.mailbox.take();
            let mutation = lease
                .fail_activation_after_zero_process(
                    fail(),
                    ZeroProcessProof::confirmed(owner("one", 1)),
                )
                .await
                .unwrap();
            assert_eq!(mutation.disposition, MutationDisposition::Duplicate);
            assert!(h.scheduler.active().is_none());
            h.finish().await;
        });
    }

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_time()
            .build()
            .unwrap()
    }

    fn ok(kind: &'static str, mutation: SessionMutation, mailbox_must_be_empty: bool) -> Step {
        let projection_revision = mutation.projection_revision;
        Step {
            kind,
            result: Ok(StoreCommit {
                projection_revision,
                results: vec![StoreMutationResult::Session(mutation)],
            }),
            mailbox_must_be_empty,
        }
    }

    fn session(
        revision_value: u64,
        state: SessionState,
        epoch: Option<u64>,
        duplicate: bool,
    ) -> SessionMutation {
        SessionMutation {
            session_id: session_id("one"),
            runtime_epoch: epoch.map(|value| RuntimeEpoch::new(value).unwrap()),
            state,
            revision: EntityRevision::new(revision_value).unwrap(),
            projection_revision: revision(revision_value),
            disposition: if duplicate {
                MutationDisposition::Duplicate
            } else {
                MutationDisposition::Applied
            },
        }
    }

    fn start() -> StartSessionCommand {
        StartSessionCommand {
            project_id: project("one"),
            chat_id: chat("one"),
            expected_projection_revision: ProjectionRevision::ZERO,
            client_intent_id: ClientIntentId::parse("start-one").unwrap(),
            binding_id: BindingId::new("binding", "fixture").unwrap(),
            binding_digest: "a".repeat(64),
            harness_version: Some(SafeText::new("version", "1").unwrap()),
            model_id: Some(ModelId::new("model", "fixture").unwrap()),
            now_millis: 1,
        }
    }

    fn activate() -> ActivateSessionCommand {
        activate_for("one")
    }

    fn activate_two() -> ActivateSessionCommand {
        activate_for("two")
    }

    fn activate_for(value: &str) -> ActivateSessionCommand {
        ActivateSessionCommand {
            project_id: project(value),
            chat_id: chat(value),
            expected_projection_revision: revision(1),
            client_intent_id: ClientIntentId::parse(format!("activate-{value}")).unwrap(),
            session_id: session_id(value),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            artifact_digest: "b".repeat(64),
            now_millis: 2,
        }
    }

    fn complete() -> CompleteActivationCommand {
        CompleteActivationCommand {
            project_id: project("one"),
            chat_id: chat("one"),
            session_id: session_id("one"),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "b".repeat(64),
            external_session_id: ExternalId::new("session", "external").unwrap(),
            capabilities_json: "{}".into(),
            now_millis: 3,
        }
    }

    fn close() -> CloseSessionCommand {
        CloseSessionCommand {
            project_id: project("one"),
            chat_id: chat("one"),
            expected_projection_revision: revision(3),
            client_intent_id: ClientIntentId::parse("close-one").unwrap(),
            session_id: session_id("one"),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            now_millis: 4,
        }
    }

    fn finalize() -> FinalizeCloseCommand {
        FinalizeCloseCommand { request: close() }
    }

    fn fail() -> FailActivationCommand {
        FailActivationCommand {
            project_id: project("one"),
            chat_id: chat("one"),
            session_id: session_id("one"),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            binding_digest: "a".repeat(64),
            artifact_digest: "b".repeat(64),
            terminal: SessionState::Failed,
            exit_kind: RequiredText::new("exit", "launch-failed").unwrap(),
            now_millis: 4,
        }
    }

    fn park() -> ParkSessionCommand {
        ParkSessionCommand {
            project_id: project("one"),
            chat_id: chat("one"),
            session_id: session_id("one"),
            runtime_epoch: RuntimeEpoch::new(1).unwrap(),
            exit_kind: RequiredText::new("exit", "idle").unwrap(),
            now_millis: 4,
        }
    }

    fn owner(value: &str, epoch: u64) -> RuntimeOwner {
        RuntimeOwner {
            project_id: project(value),
            chat_id: chat(value),
            session_id: session_id(value),
            runtime_epoch: RuntimeEpoch::new(epoch).unwrap(),
        }
    }

    fn project(value: &str) -> SageProjectId {
        SageProjectId::parse(format!("project-{value}")).unwrap()
    }

    fn chat(value: &str) -> SageChatId {
        SageChatId::parse(format!("chat-{value}")).unwrap()
    }

    fn session_id(value: &str) -> SageAgentSessionId {
        SageAgentSessionId::parse(format!("session-{value}")).unwrap()
    }

    fn revision(value: u64) -> ProjectionRevision {
        ProjectionRevision::new(value).unwrap()
    }

    fn kind(mutation: &StoreMutation) -> &'static str {
        match mutation {
            StoreMutation::StartSession(_) => "start",
            StoreMutation::ActivateSession(_) => "activate",
            StoreMutation::CompleteActivation(_) => "complete",
            StoreMutation::FailActivation(_) => "fail",
            StoreMutation::CloseSession(_) => "close",
            StoreMutation::FinalizeClose(_) => "finalize",
            StoreMutation::ParkSession(_) => "park",
            _ => "unexpected",
        }
    }
}
