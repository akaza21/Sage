//! Service entry points that route one exact runtime owner to its live lane.
//!
//! An owner no lane holds is stale or foreign; it is refused here without
//! touching any lane.

use std::{sync::Arc, time::Instant};

use crate::acp::{
    runtime::{HostCancelIntent, HostError, HostOutcome, HostPromptIntent, RuntimeHost},
    scheduler::RuntimeOwner,
    store::{ActivateSessionCommand, CloseSessionCommand},
};

use super::{AcpService, CodexSessionError, PromptSendError};

impl AcpService {
    pub(crate) async fn activate_session(
        &self,
        admitted: crate::acp::bindings::registry::AdmittedBindingRecord<'_>,
        command: ActivateSessionCommand,
        project_root: crate::acp::bindings::registry::BindingProjectRoot,
    ) -> Result<(), CodexSessionError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        let deadlines = super::codex_live::deadlines()?;
        let (lane, _claim) = self.claim_activation_lane(&owner, deadlines.1)?;
        lane.activate_session(admitted, command, project_root, deadlines)
            .await
    }

    pub(crate) fn send_session_turn(
        &self,
        intent: HostPromptIntent,
    ) -> Result<HostOutcome, PromptSendError<CodexSessionError>> {
        self.lane_for(&intent.owner())
            .ok_or_else(|| PromptSendError::not_started(CodexSessionError::Intent))?
            .send_session_turn(intent)
    }

    pub(crate) fn cancel_session_turn(
        &self,
        intent: HostCancelIntent,
    ) -> Result<HostOutcome, CodexSessionError> {
        match self.lane_for(&intent.owner) {
            Some(lane) => lane.cancel_session_turn(intent),
            // The runtime already converged (for example after a refused
            // prompt); Cancel of the settled turn is satisfied, not stale.
            None if self.turn_is_terminal(&intent.owner, &intent.turn_id) => {
                Ok(HostOutcome::CancelQueued)
            }
            None => Err(CodexSessionError::Intent),
        }
    }

    pub(in crate::acp::service) fn turn_is_terminal(
        &self,
        owner: &RuntimeOwner,
        turn_id: &crate::acp::contract::SageTurnId,
    ) -> bool {
        self.load_chat_page(owner.project_id.clone(), owner.chat_id.clone(), None)
            .is_ok_and(|page| {
                page.turns.as_slice().iter().any(|turn| {
                    turn.id == *turn_id
                        && turn.session_id == owner.session_id
                        && turn.state.is_terminal()
                })
            })
    }

    pub(crate) fn close_session(
        &self,
        command: CloseSessionCommand,
    ) -> Result<HostOutcome, CodexSessionError> {
        let owner = RuntimeOwner {
            project_id: command.project_id.clone(),
            chat_id: command.chat_id.clone(),
            session_id: command.session_id.clone(),
            runtime_epoch: command.runtime_epoch,
        };
        self.lane_for(&owner)
            .ok_or(CodexSessionError::Intent)?
            .close_session(command)
    }

    pub(crate) fn end_session_natively(
        &self,
        owner: RuntimeOwner,
    ) -> Result<(), CodexSessionError> {
        self.lane_for(&owner)
            .ok_or(CodexSessionError::Intent)?
            .end_session_natively(owner)
    }

    pub(crate) fn dispose_temporary_runtime(
        &self,
        project: &crate::acp::contract::SageProjectId,
        chat: &crate::acp::contract::SageChatId,
    ) -> Result<(), CodexSessionError> {
        if !crate::side_chat_storage::is_temporary(&self.database.connection(), chat.as_str())
            .map_err(|_| CodexSessionError::Recovery)?
        {
            return Err(CodexSessionError::Intent);
        }
        self.file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear_chat(project.as_str(), chat.as_str());
        self.revoke_browser_chat(project, chat);
        let lane = self.lanes().find(|lane| {
            lane.scheduler.active().is_some_and(|active| {
                &active.tree.project_id == project && &active.tree.chat_id == chat
            })
        });
        match lane {
            Some(lane) => lane.dispose_temporary_runtime(project, chat),
            None => Ok(()),
        }
    }

    pub(in crate::acp::service) fn retain_config_effect_recovery(
        &self,
        owner: RuntimeOwner,
        host: Arc<RuntimeHost>,
    ) {
        if let Some(lane) = self.lane_for(&owner) {
            lane.retain_config_effect_recovery(owner, host);
        }
    }

    /// Drives every lane's retained recovery once. It reports whether any
    /// lane reclaimed capacity and fails if any lane still retains authority.
    pub(in crate::acp::service) fn shutdown_codex_recovery(
        &self,
        deadline: Instant,
    ) -> Result<bool, HostError> {
        let mut reclaimed = false;
        let mut failure = None;
        for lane in self.lanes() {
            match lane.shutdown_codex_recovery(deadline) {
                Ok(lane_reclaimed) => reclaimed |= lane_reclaimed,
                Err(error) => {
                    failure.get_or_insert(error);
                }
            }
        }
        failure.map_or(Ok(reclaimed), Err)
    }

    pub(in crate::acp::service) fn has_retained_recovery(&self) -> bool {
        self.lanes().any(|lane| {
            lane.codex_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        })
    }

    #[cfg(test)]
    pub(crate) fn reclaim_zero_process_capacity(
        &self,
        deadline: Instant,
    ) -> Result<bool, HostError> {
        let mut reclaimed = false;
        for lane in self.lanes() {
            reclaimed |= lane.reclaim_zero_process_capacity(deadline)?;
        }
        Ok(reclaimed)
    }

    #[cfg(test)]
    pub(crate) fn send_codex_turn(
        &self,
        command: crate::acp::store::PrepareTurnCommand,
    ) -> Result<HostOutcome, CodexSessionError> {
        self.send_session_turn(HostPromptIntent {
            command,
            access_mode: crate::settings_store::SessionAccessMode::ReadOnly,
        })
        .map_err(|failure| failure.error)
    }

    #[cfg(test)]
    pub(crate) fn cancel_codex_turn(
        &self,
        intent: HostCancelIntent,
    ) -> Result<HostOutcome, CodexSessionError> {
        #[cfg(windows)]
        let started = std::time::Instant::now();
        #[cfg(windows)]
        let owner = intent.owner.clone();
        let result = self.cancel_session_turn(intent);
        #[cfg(windows)]
        if let Err(error) = &result {
            let host = self.host_for(&owner).ok().map(|host| {
                (
                    host.lifecycle.load(std::sync::atomic::Ordering::Acquire),
                    host.last_shutdown_phase(),
                )
            });
            eprintln!(
                "ACP cancellation failed: stage={} elapsed_ms={} error={error:?} slot={:?} host={host:?} scheduler={:?}",
                self.last_cancel_stage(),
                started.elapsed().as_millis(),
                self.host_for(&owner).err(),
                self.lane_for(&owner)
                    .and_then(|lane| lane.scheduler.active())
                    .map(|allocation| (allocation.turn_reserved, allocation.turn_id)),
            );
        }
        result
    }

    #[cfg(test)]
    pub(crate) fn close_codex_session(
        &self,
        command: CloseSessionCommand,
    ) -> Result<HostOutcome, CodexSessionError> {
        self.close_session(command)
    }

    #[cfg(all(test, windows))]
    pub(crate) async fn activate_codex_session(
        &self,
        command: ActivateSessionCommand,
        project_root: crate::acp::bindings::codex::CodexProjectRootAuthority,
    ) -> Result<(), CodexSessionError> {
        let admitted = self
            .refresh_codex_binding()
            .map_err(CodexSessionError::Binding)?;
        self.activate_session(
            admitted,
            command,
            crate::acp::bindings::registry::BindingProjectRoot::Codex(project_root),
        )
        .await
    }

    #[cfg(all(test, windows))]
    pub(crate) fn fail_next_runtime_port_finish_park(&self) {
        *self
            .runtime_port_finish_park_failure
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
    }

    #[cfg(all(test, windows))]
    pub(crate) fn has_retained_live_host_recovery_for(&self, owner: &RuntimeOwner) -> bool {
        self.lanes()
            .any(|lane| lane.has_retained_live_host_recovery_for(owner))
    }
}
