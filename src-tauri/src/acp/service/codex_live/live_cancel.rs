use super::*;

impl AcpService {
    pub(crate) fn park_after_cancel_zero(
        &self,
        owner: &RuntimeOwner,
    ) -> Result<(), CodexSessionError> {
        let exit_kind = crate::acp::contract::RequiredText::new("exitKind", "hostCancel")
            .map_err(|_| CodexSessionError::Recovery)?;
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
        .map_err(|_| CodexSessionError::Recovery)?;
        self.scheduler
            .recover_owner_after_zero_process(owner)
            .map_err(|_| CodexSessionError::Recovery)?;
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
            Some(PendingCodexRecovery::LiveHost(recovery)) if &recovery.owner != owner => {
                *retained = Some(PendingCodexRecovery::LiveHost(recovery));
            }
            Some(
                recovery @ (PendingCodexRecovery::Runner { .. }
                | PendingCodexRecovery::LiveHost(_)),
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
