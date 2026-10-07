use super::live_recovery::{LiveHostRecovery, LiveHostRecoveryPhase};
use super::*;

impl AcpService {
    pub(crate) fn park_idle_runtime_for_replacement(
        &self,
        deadline: std::time::Instant,
    ) -> Result<bool, CodexSessionError> {
        let Some(allocation) = self.scheduler.active() else {
            return Ok(false);
        };
        if allocation.turn_reserved || !self.runtime_has_completed_turn(&allocation.tree) {
            return Ok(false);
        }
        let owner = allocation.tree;
        let Ok(host) = self.runtime.for_owner(&owner) else {
            return Ok(false);
        };
        if !host.has_shutdown_recovery() {
            return Ok(false);
        }
        if self.scheduler.begin_idle_drain(&owner).is_err() {
            return Ok(false);
        }
        if let Err(error) = self.runtime.mark_recovery(&owner) {
            let _ = self.scheduler.cancel_idle_drain(&owner);
            return Err(map_slot_error(error));
        }
        if host.shutdown(deadline).is_ok()
            && self.scheduler.active().is_none()
            && self.runtime.release_recovered(&owner, &host).is_ok()
        {
            return Ok(true);
        }
        self.retain_codex_recovery(PendingCodexRecovery::LiveHost(LiveHostRecovery {
            owner,
            host,
            phase: LiveHostRecoveryPhase::ReplacementPark,
        }));
        Err(CodexSessionError::Recovery)
    }

    fn runtime_has_completed_turn(&self, owner: &RuntimeOwner) -> bool {
        self.database
            .connection()
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM turns WHERE session_id=?1 AND runtime_epoch=?2 AND state='completed')",
                rusqlite::params![
                    owner.session_id.as_str(),
                    owner.runtime_epoch.get() as i64
                ],
                |row| row.get::<_, i64>(0),
            )
            .is_ok_and(|exists| exists != 0)
    }

    pub(crate) fn park_after_cancel_zero(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), CodexSessionError> {
        let exit_kind = crate::acp::contract::RequiredText::new("exitKind", "hostCancel")
            .map_err(|_| CodexSessionError::Recovery)?;
        #[cfg(all(test, windows))]
        self.record_cancel_stage("park-store");
        crate::acp::store::park_session_after_zero_process(
            &mut self.database.connection(),
            crate::acp::store::ParkSessionCommand {
                project_id: owner.project_id.clone(),
                chat_id: owner.chat_id.clone(),
                session_id: owner.session_id.clone(),
                runtime_epoch: owner.runtime_epoch,
                exit_kind,
                now_millis: crate::persistence::now_millis(),
            },
        )
        .map_err(|_error| {
            #[cfg(all(test, windows))]
            self.record_cancel_stage(match _error {
                crate::acp::store::StoreError::Conflict { .. } => "park-store-conflict",
                crate::acp::store::StoreError::InvalidTransition { .. } => {
                    "park-store-invalid-transition"
                }
                crate::acp::store::StoreError::TerminalConflict { .. } => {
                    "park-store-terminal-conflict"
                }
                crate::acp::store::StoreError::Database(_) => "park-store-database",
                _ => "park-store-other",
            });
            CodexSessionError::Recovery
        })?;
        #[cfg(all(test, windows))]
        self.record_cancel_stage("park-scheduler");
        self.scheduler
            .recover_owner_after_zero_process(owner)
            .map_err(|_| CodexSessionError::Recovery)?;
        #[cfg(all(test, windows))]
        self.record_cancel_stage("park-forget");
        self.forget_stale_owner_recovery(owner);
        Ok(())
    }

    pub(crate) fn forget_stale_owner_recovery(&self, owner: &RuntimeOwner) {
        let mut retained = self
            .codex_recovery
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match retained.take() {
            Some(PendingCodexRecovery::Runner {
                owner: current,
                runner,
            }) if &current != owner => {
                *retained = Some(PendingCodexRecovery::Runner {
                    owner: current,
                    runner,
                });
            }
            Some(PendingCodexRecovery::LiveHost(recovery))
                if &recovery.owner != owner
                    || matches!(
                        recovery.phase,
                        LiveHostRecoveryPhase::ReplacementPark
                            | LiveHostRecoveryPhase::NativeEnd
                            | LiveHostRecoveryPhase::NativeEndInFlight
                    ) =>
            {
                *retained = Some(PendingCodexRecovery::LiveHost(recovery));
            }
            Some(
                recovery
                @ (PendingCodexRecovery::Runner { .. } | PendingCodexRecovery::LiveHost(_)),
            ) => {
                drop(recovery);
            }
            Some(other) => *retained = Some(other),
            None => {}
        }
    }

    pub(crate) fn cancel_is_durable(
        &self,
        owner: &RuntimeOwner,
        turn_id: &crate::acp::contract::SageTurnId,
    ) -> bool {
        self.load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .is_ok_and(|page| {
                page.turns.as_slice().iter().any(|turn| {
                    turn.id == *turn_id
                        && turn.session_id == owner.session_id
                        && matches!(
                            turn.state,
                            crate::acp::lifecycle::TurnState::Cancelled
                                | crate::acp::lifecycle::TurnState::Interrupted
                        )
                })
            })
    }
}
