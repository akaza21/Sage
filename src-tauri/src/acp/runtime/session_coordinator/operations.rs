use super::*;

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

    /// Final-window recovery after native supervision has proved this exact
    /// runtime tree is zero. Durable active state is terminalized with the
    /// same restart-recovery transaction; scheduler capacity is advertised
    /// only after that commit is authoritative.
    // The one-shot proof is retained in the error until durable recovery,
    // publication, and scheduler release all complete.
    #[allow(clippy::result_large_err)]
    pub(crate) fn recover_runtime_after_zero_process_retained(
        &self,
        owner: RuntimeOwner,
        proof: ZeroProcessProof,
    ) -> Result<ZeroProcessProof, (SessionCoordinatorError, ZeroProcessProof)> {
        if proof.owner != owner {
            return Err((SessionCoordinatorError::Intent, proof));
        }
        let ticket = match self.journal.enqueue_one(
            JournalLane::Control,
            StoreMutation::RecoverRuntimeAfterZero(RecoverRuntimeCommand {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                now_millis: crate::persistence::now_millis(),
            }),
            deadline(JournalLane::Control),
        ) {
            Ok(ticket) => ticket,
            Err(error) => return Err((error.into(), proof)),
        };
        let commit = match ticket.blocking_committed() {
            Ok(commit) => commit,
            Err(error) => return Err((error.into(), proof)),
        };
        let revision = match commit.results.as_slice() {
            [StoreMutationResult::Recovery(report)]
                if report.recovered_sessions <= 1
                    && report.projection_revision == Some(commit.projection_revision) =>
            {
                commit.projection_revision
            }
            _ => return Err((SessionCoordinatorError::JournalProtocol, proof)),
        };
        let disposition = match self.publisher.publish_committed(ProjectionChangedV1 {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            session_id: owner.session_id.clone(),
            projection_revision: revision,
            child_projection_epoch: 0,
        }) {
            Ok(disposition) => disposition,
            Err(error) => return Err((error.into(), proof)),
        };
        if matches!(
            disposition,
            ProjectionPublishStatus::BootstrapRequired | ProjectionPublishStatus::RecoveryRequired
        ) {
            return Err((SessionCoordinatorError::BootstrapRequired, proof));
        }
        if let Err(error) = self.scheduler.recover_owner_after_zero_process(&owner) {
            return Err((error.into(), proof));
        }
        Ok(proof)
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
            #[cfg(feature = "acp-fixture")]
            characterization_observation: None,
        })
    }

    pub(super) async fn commit(
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

    pub(super) fn commit_blocking(
        &self,
        lane: JournalLane,
        mutation: StoreMutation,
    ) -> Result<SessionMutation, SessionCoordinatorError> {
        let commit = self
            .journal
            .enqueue_one(lane, mutation, deadline(lane))?
            .blocking_committed()?;
        one_session(commit)
    }

    pub(super) fn publish(
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
            child_projection_epoch: 0,
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
