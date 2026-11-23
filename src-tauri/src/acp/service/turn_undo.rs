use super::{file_rewind, *};
use crate::acp::{ipc::ProjectionChangedV1, store::UndoneThread, turn_undo::stage_turn_undo};
use sage_acp_protocol::{contract::SageTurnId, scheduler::RuntimeOwner};

mod route;
use route::Rewind;

impl AcpService {
    /// How the running session `owner` would undo `turn`, its chat's latest
    /// visible finished turn, or `None` when it cannot.
    pub(super) fn turn_undo(
        &self,
        owner: &RuntimeOwner,
        capabilities: &sage_acp_protocol::intent::CapabilitySnapshotV1,
        turn: &str,
    ) -> Result<Option<TurnUndo>, StoreError> {
        let Some(undo) = TurnUndo::from_capability(capabilities.rewind) else {
            return Ok(None);
        };
        let withheld = crate::acp::store::turn_undo::undo_withheld(
            &self.database.connection(),
            owner.session_id.as_str(),
            turn,
            capabilities
                .rewind
                .runtime_checkpoint
                .then_some(owner.runtime_epoch.get()),
        )?;
        Ok((!withheld).then_some(undo))
    }

    /// Whether `owner` may undo `turn`: by the same rule the page offers it.
    pub(crate) fn undoable_turn(
        &self,
        owner: &RuntimeOwner,
        turn: &SageTurnId,
    ) -> Result<Option<TurnUndo>, StoreError> {
        let (project, chat, session) = (
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
        );
        let connection = self.database.connection();
        crate::acp::store::turn_undo::intent::ensure_settled(&connection, project, chat)?;
        if !crate::acp::store::session_surface::is_undoable_turn(
            &connection,
            project,
            chat,
            session,
            turn.as_str(),
        )? {
            return Ok(None);
        }
        let capabilities: String = connection.query_row(
            "SELECT s.capabilities_json FROM agent_sessions s
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3",
            rusqlite::params![project, chat, session],
            |row| row.get(0),
        )?;
        drop(connection);
        let capabilities = serde_json::from_str(&capabilities)
            .map_err(|_| StoreError::Corrupt("rewind capabilities"))?;
        self.turn_undo(owner, &capabilities, turn.as_str())
    }

    /// Persist the resolved plan before native IO, then settle the effect and
    /// visibility in one store transaction. An unknown result keeps the intent
    /// pending and hands the exact runtime to the existing recovery machinery.
    /// With `keep_files`, only the conversation is cut and files stay.
    pub(crate) async fn undo_turn(
        &self,
        owner: &RuntimeOwner,
        turn_id: SageTurnId,
        keep_files: bool,
    ) -> Result<Option<sage_acp_protocol::contract::SafeText<512>>, SessionControlError> {
        self.apply_rewind(owner, turn_id, Rewind::Undo { keep_files })
            .await
    }

    /// With `files`, the files changed since `before` are restored first,
    /// after a dry run and a check that none changed since the agent wrote it.
    pub(crate) async fn rewind_conversation(
        &self,
        owner: &RuntimeOwner,
        before: SageTurnId,
        files: bool,
    ) -> Result<Option<sage_acp_protocol::contract::SafeText<512>>, SessionControlError> {
        self.apply_rewind(owner, before, Rewind::Conversation { files })
            .await
    }

    async fn apply_rewind(
        &self,
        owner: &RuntimeOwner,
        turn_id: SageTurnId,
        mut mode: Rewind,
    ) -> Result<Option<sage_acp_protocol::contract::SafeText<512>>, SessionControlError> {
        let claim = self.claim_undo_lane(owner)?;
        crate::acp::store::turn_undo::intent::ensure_settled(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
        )
        .map_err(|_| SessionControlError::RecoveryRequired(None))?;

        let undo = if let Rewind::Conversation { files } = mode {
            let json: String = self
                .database
                .connection()
                .query_row(
                    "SELECT capabilities_json FROM agent_sessions WHERE id=?1",
                    [owner.session_id.as_str()],
                    |r| r.get(0),
                )
                .map_err(|_| SessionControlError::RecoveryRequired(None))?;
            let capabilities: sage_acp_protocol::intent::CapabilitySnapshotV1 =
                serde_json::from_str(&json).map_err(|_| SessionControlError::Unavailable)?;
            if capabilities.rewind.targets == sage_acp_protocol::intent::RewindTargets::LatestTurn {
                // A latest-turn offer rewinds by undo; with files it stays
                // unavailable (see `rewind_turn`).
                if !files {
                    mode = Rewind::Undo { keep_files: false };
                }
                self.undoable_turn(owner, &turn_id)
                    .map_err(|_| SessionControlError::RecoveryRequired(None))?
                    .ok_or(SessionControlError::Unavailable)?
            } else {
                TurnUndo::from_capability(capabilities.rewind)
                    .ok_or(SessionControlError::Unavailable)?
            }
        } else {
            self.undoable_turn(owner, &turn_id)
                .map_err(|_| SessionControlError::RecoveryRequired(None))?
                .ok_or(SessionControlError::Unavailable)?
        };
        let (binding, external): (String, String) = self
            .database
            .connection()
            .query_row(
                "SELECT binding_id,external_session_id FROM agent_sessions WHERE id=?1",
                [owner.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| SessionControlError::RecoveryRequired(None))?;
        let binding = sage_acp_protocol::intent::BindingId::new("bindingId", binding)
            .map_err(|_| SessionControlError::RecoveryRequired(None))?;
        let resolve_cut = || {
            self.resolve_session_binding(&binding)
                .map_err(|_| "The conversation transcript is unavailable.".to_owned())?
                .undo_cut(&external, turn_id.as_str())
                .map_err(|error| error.to_string())
        };
        let (command, summary, restored) = self
            .rewind_turn(
                owner,
                &turn_id,
                undo,
                mode,
                &mut |control| self.session_control_reply(owner.clone(), control),
                resolve_cut,
            )
            .await?;
        let recorded = self
            .database
            .write_connection()
            .ok()
            .and_then(|mut connection| {
                crate::acp::store::record_turn_undone(&mut connection, &command).ok()
            });
        let Some(recorded) = recorded else {
            self.recover_undo_runtime(owner);
            return Err(file_rewind::staged_error(
                restored,
                SessionControlError::RecoveryRequired(None),
            ));
        };
        if let Some(projection_revision) = recorded.published {
            // Delivery is advisory: the page reloads from SQLite either way.
            let _ = self.publisher.publish_committed(ProjectionChangedV1 {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                projection_revision,
                child_projection_epoch: 0,
            });
        }
        drop(claim);
        if recorded.thread == UndoneThread::Reverted || self.runs_background_tasks(owner) {
            return Ok(summary);
        }
        if let Err(refusal) = self.park_undone_runtime(owner).await {
            tracing::warn!(?refusal, "the runtime that undid a turn is not yet parked");
        }
        Ok(summary)
    }

    /// Resolve target eligibility only for the new conversation operation.
    /// Last-turn undo retains the binding's own checkpoint/identity resolver.
    fn prepare_rewind<E: std::fmt::Display>(
        &self,
        owner: &RuntimeOwner,
        turn_id: SageTurnId,
        undo: TurnUndo,
        conversation_only: bool,
        resolve_cut: impl FnOnce() -> Result<Option<String>, E>,
    ) -> Result<
        (
            crate::acp::store::TurnUndoneCommand,
            sage_acp_protocol::session_control::SessionControl,
        ),
        SessionControlError,
    > {
        let control = if conversation_only {
            let target = crate::acp::store::rewind::target(
                &self.database.connection(),
                owner.project_id.as_str(),
                owner.chat_id.as_str(),
                owner.session_id.as_str(),
                turn_id.as_str(),
            )
            .map_err(|error| match error {
                StoreError::RewindUnavailable { reason } => SessionControlError::Refused(
                    sage_acp_protocol::contract::SafeText::new("controlSummary", reason).ok(),
                ),
                _ => SessionControlError::Unavailable,
            })?;
            sage_acp_protocol::session_control::SessionControl::RewindConversation {
                before: target,
            }
        } else {
            sage_acp_protocol::session_control::SessionControl::UndoTurn {
                turn_id: turn_id.clone(),
            }
        };
        let command = stage_turn_undo(
            &self.database,
            owner,
            turn_id,
            undo.reverts_thread,
            conversation_only,
            resolve_cut,
        )?;
        Ok((command, control))
    }

    /// A staged cut whose native effect was refused is rolled back; any
    /// other failure leaves the outcome unknown, for recovery to settle.
    pub(super) fn abandon_staged(
        &self,
        owner: &RuntimeOwner,
        command: &crate::acp::store::TurnUndoneCommand,
        error: SessionControlError,
    ) -> SessionControlError {
        if matches!(
            error,
            SessionControlError::Refused(_) | SessionControlError::Unavailable
        ) && self
            .database
            .write_connection()
            .ok()
            .and_then(|mut connection| {
                crate::acp::store::turn_undo::intent::rollback(&mut connection, command).ok()
            })
            .is_some()
        {
            return error;
        }
        self.recover_undo_runtime(owner);
        SessionControlError::RecoveryRequired(None)
    }

    fn recover_undo_runtime(&self, owner: &RuntimeOwner) {
        if let Some(lane) = self.lane_for(owner)
            && let Ok(host) = lane.runtime.for_owner(owner)
        {
            self.retain_config_effect_recovery(owner.clone(), host);
        }
    }

    /// Whether the runtime `owner` still holds a turn the user undid. A
    /// rewind that cannot be read counts as held, so no prompt reaches a
    /// runtime that might hold one.
    pub(crate) fn holds_undone_turn(&self, owner: &RuntimeOwner) -> Result<bool, StoreError> {
        crate::acp::store::turn_undo::intent::ensure_settled(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
        )?;
        let rewind = crate::acp::store::pending_thread_rewind(
            &self.database.connection(),
            owner.session_id.as_str(),
        )?;
        Ok(rewind.is_some_and(|rewind| rewind.held_by(owner.runtime_epoch)))
    }

    fn runs_background_tasks(&self, owner: &RuntimeOwner) -> bool {
        crate::acp::store::session_surface::view(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            Some(owner.runtime_epoch.get()),
        )
        // Unreadable, the surface is treated as busy: the park is only
        // deferred to the next send, which relaunches either way.
        .map_or(true, |view| {
            view.is_some_and(|view| {
                view.activities
                    .iter()
                    .any(|activity| !activity.state.is_terminal())
            })
        })
    }

    async fn park_undone_runtime(&self, owner: &RuntimeOwner) -> Result<(), ChatRuntimeRelease> {
        self.release_chat_runtime_until(
            &owner.project_id,
            &owner.chat_id,
            super::explicit_close_deadline(),
            || self.holds_undone_turn(owner).unwrap_or(true),
        )
        .await
    }

    pub(crate) async fn relaunch_without_undone_turn(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        owner: &RuntimeOwner,
        generation: sage_acp_protocol::contract::ProjectGeneration,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<RuntimeOwner, UndoneRelaunchError> {
        self.park_undone_runtime(owner)
            .await
            .map_err(UndoneRelaunchError::Parking)?;
        let snapshot = self
            .intent_snapshot(owner.project_id.clone(), generation, owner.chat_id.clone())
            .map_err(|error| UndoneRelaunchError::Activation(session::store_error(error)))?;
        let command = crate::acp::store::ActivateSessionCommand {
            project_id: owner.project_id.clone(),
            chat_id: owner.chat_id.clone(),
            expected_projection_revision: snapshot.projection_revision,
            client_intent_id: sage_acp_protocol::contract::ClientIntentId::new(),
            session_id: owner.session_id.clone(),
            runtime_epoch: session::next_runtime_epoch(&self.database, &owner.session_id)
                .map_err(UndoneRelaunchError::Activation)?,
            artifact_digest: String::new(),
            now_millis: sage_core::time::now_millis(),
        };
        self.activate_head_session(admitted, command, project_root)
            .await
            .map_err(UndoneRelaunchError::Activation)
    }
}

#[derive(Debug)]
pub(crate) enum UndoneRelaunchError {
    Parking(ChatRuntimeRelease),
    Activation(CodexSessionError),
}

/// Last-turn effects and native execution strategy come from the binding's offer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnUndo {
    pub(super) keeps_files: bool,
    /// The harness applies the conversation cut itself rather than on resume.
    reverts_thread: bool,
}

impl TurnUndo {
    fn from_capability(capability: sage_acp_protocol::intent::RewindCapability) -> Option<Self> {
        capability.effects.conversation.then_some(Self {
            keeps_files: !capability.effects.files,
            reverts_thread: !capability.on_resume,
        })
    }
}

#[cfg(test)]
#[path = "turn_undo_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "turn_undo_files_tests.rs"]
mod files_tests;
