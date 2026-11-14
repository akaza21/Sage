use super::*;
use crate::acp::{
    contract::SageTurnId, intent::ProjectionChangedV1, scheduler::RuntimeOwner,
    store::UndoneThread, turn_undo::stage_turn_undo,
};

impl AcpService {
    /// How the running session `owner` would undo `turn`, its chat's latest
    /// visible finished turn, or `None` when it cannot.
    pub(super) fn turn_undo(
        &self,
        owner: &RuntimeOwner,
        binding: &str,
        turn: &str,
        turn_epoch: u64,
    ) -> Result<Option<TurnUndo>, StoreError> {
        let Some(undo) =
            TurnUndo::of(binding).filter(|undo| undo.offers(owner.runtime_epoch.get(), turn_epoch))
        else {
            return Ok(None);
        };
        let withheld = crate::acp::store::turn_undo::undo_withheld(
            &self.database.connection(),
            owner.project_id.as_str(),
            owner.chat_id.as_str(),
            owner.session_id.as_str(),
            turn,
            undo.reverts_thread,
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
        let (binding, turn_epoch): (String, i64) = connection.query_row(
            "SELECT s.binding_id,t.runtime_epoch FROM agent_sessions s
             JOIN turns t ON t.session_id=s.id AND t.id=?4
             WHERE s.project_id=?1 AND s.chat_id=?2 AND s.id=?3",
            rusqlite::params![project, chat, session, turn.as_str()],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        drop(connection);
        let turn_epoch =
            u64::try_from(turn_epoch).map_err(|_| StoreError::Corrupt("turn epoch"))?;
        self.turn_undo(owner, &binding, turn.as_str(), turn_epoch)
    }

    /// Persist the resolved plan before native IO, then settle the effect and
    /// visibility in one store transaction. An unknown result keeps the intent
    /// pending and hands the exact runtime to the existing recovery machinery.
    pub(crate) async fn undo_turn(
        &self,
        owner: &RuntimeOwner,
        turn_id: SageTurnId,
    ) -> Result<Option<crate::acp::contract::SafeText<512>>, SessionControlError> {
        let claim = self.claim_undo_lane(owner)?;
        let unknown = |_| SessionControlError::RecoveryRequired;
        let undo = self
            .undoable_turn(owner, &turn_id)
            .map_err(unknown)?
            .ok_or(SessionControlError::Unavailable)?;
        let (binding, external): (String, String) = self
            .database
            .connection()
            .query_row(
                "SELECT binding_id,external_session_id FROM agent_sessions WHERE id=?1",
                [owner.session_id.as_str()],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|_| SessionControlError::RecoveryRequired)?;
        let binding = crate::acp::intent::BindingId::new("bindingId", binding)
            .map_err(|_| SessionControlError::RecoveryRequired)?;
        let command = stage_turn_undo(
            &self.database,
            owner,
            turn_id.clone(),
            undo.reverts_thread,
            || {
                self.resolve_session_binding(&binding)
                    .map_err(|_| crate::acp::bindings::claude::sdk::TranscriptError::Unreadable)?
                    .undo_cut(&external, turn_id.as_str())
            },
        )?;
        let summary = match self.control_session(
            owner.clone(),
            crate::acp::session_control::SessionControl::UndoTurn { turn_id },
        ) {
            Ok(summary) => summary,
            Err(error) => {
                if matches!(
                    error,
                    SessionControlError::Refused(_) | SessionControlError::Unavailable
                ) {
                    let rollback =
                        self.database
                            .write_connection()
                            .ok()
                            .and_then(|mut connection| {
                                crate::acp::store::turn_undo::intent::rollback(
                                    &mut connection,
                                    &command,
                                )
                                .ok()
                            });
                    if rollback.is_some() {
                        return Err(error);
                    }
                }
                self.recover_undo_runtime(owner);
                return Err(SessionControlError::RecoveryRequired);
            }
        };
        let recorded = self
            .database
            .write_connection()
            .ok()
            .and_then(|mut connection| {
                crate::acp::store::record_turn_undone(&mut connection, &command).ok()
            });
        let Some(recorded) = recorded else {
            self.recover_undo_runtime(owner);
            return Err(SessionControlError::RecoveryRequired);
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
        generation: crate::acp::contract::ProjectGeneration,
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
            client_intent_id: crate::acp::contract::ClientIntentId::new(),
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

/// Claude Code checkpoints each turn's files and restores them, and has no
/// control that drops a turn from its thread, so Sage cuts the thread on its
/// next launch; Codex drops the turn from its thread itself and leaves files
/// as they are.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct TurnUndo {
    pub(super) keeps_files: bool,
    /// The harness drops the turn from its own thread, which only the
    /// process that ran the turn holds, and only its last turn at that.
    reverts_thread: bool,
}

impl TurnUndo {
    fn of(binding: &str) -> Option<Self> {
        if binding == crate::acp::bindings::claude::sdk::CLAUDE_SDK_BINDING_ID {
            Some(Self {
                keeps_files: false,
                reverts_thread: false,
            })
        } else if binding == crate::acp::bindings::codex::CODEX_BINDING_ID {
            Some(Self {
                keeps_files: true,
                reverts_thread: true,
            })
        } else {
            None
        }
    }

    fn offers(self, session_epoch: u64, turn_epoch: u64) -> bool {
        !self.reverts_thread || session_epoch == turn_epoch
    }
}

#[cfg(test)]
#[path = "turn_undo_tests.rs"]
mod tests;
