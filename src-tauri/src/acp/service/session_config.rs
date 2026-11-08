use super::*;
use crate::acp::{
    contract::BoundedVec,
    scheduler::RuntimeOwner,
    session_config::{ConfigIntent, NoticeSink},
};

mod restore;
#[cfg(test)]
mod restore_tests;
#[cfg(test)]
mod tests;

impl AcpService {
    pub(crate) fn set_session_config_notice_sink(&self, sink: NoticeSink) {
        for lane in self.lanes() {
            lane.runtime.config.set_sink(sink.clone());
        }
    }
    pub(crate) fn configure_session(
        &self,
        intent: ConfigIntent,
    ) -> Result<(), crate::acp::runtime::HostError> {
        let lane = self
            .lane_for(&intent.owner)
            .ok_or(crate::acp::runtime::HostError::Intent)?;
        let host = lane
            .runtime
            .for_owner(&intent.owner)
            .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        lane.runtime
            .config
            .validate(&intent)
            .map_err(|_| crate::acp::runtime::HostError::Intent)?;
        let result = host.configure_session(intent.clone());
        if result.is_ok() {
            // The harness confirmed this choice: it is what a resumed
            // runtime of this session restores.
            self.remember_config_choice(&intent.owner, &intent.mutation);
        }
        if matches!(
            result,
            Err(crate::acp::runtime::HostError::Unavailable
                | crate::acp::runtime::HostError::Closed
                | crate::acp::runtime::HostError::Deadline
                | crate::acp::runtime::HostError::DeliveryUnknown
                | crate::acp::runtime::HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(intent.owner.clone(), host);
        }
        result.map(|_| ())
    }
    pub(super) fn compose_session_config(&self, page: &mut AcpChatPage) -> Result<(), StoreError> {
        page.session_config = None;
        let mut sessions = page.sessions.as_slice().to_vec();
        for session in &mut sessions {
            session.capabilities.config_count = 0;
            if Some(&session.id) != page.current_session_id.as_ref()
                || session.residency != crate::acp::contract::Residency::Hot
                || !matches!(
                    session.state,
                    crate::acp::contract::SessionState::Ready
                        | crate::acp::contract::SessionState::Active
                )
            {
                continue;
            }
            let Some(runtime_epoch) = session.runtime_epoch else {
                continue;
            };
            let owner = RuntimeOwner {
                project_id: page.project_id.clone(),
                chat_id: page.chat_id.clone(),
                session_id: session.id.clone(),
                runtime_epoch,
            };
            let Some(lane) = self.lane_for(&owner) else {
                continue;
            };
            if matches!(
                lane.runtime.for_owner(&owner),
                Ok(_) | Err(super::runtime_slot::RuntimeSlotError::Recovery)
            ) {
                page.session_config = lane.runtime.config.snapshot(&owner);
                session.capabilities.config_count = page
                    .session_config
                    .as_ref()
                    .filter(|c| !c.removed)
                    .map_or(0, |c| c.config.config_options.as_slice().len() as u16);
            }
        }
        page.sessions = BoundedVec::new("sessions", sessions)
            .map_err(|_| StoreError::Corrupt("session config page"))?;
        let bytes = serde_json::to_vec(page)
            .map_err(|_| StoreError::Corrupt("session config page"))?
            .len();
        if bytes > crate::acp::limits::LIMITS.durable.history_page_bytes as usize {
            return Err(StoreError::Capacity {
                resource: "historyPage",
                limit: crate::acp::limits::LIMITS.durable.history_page_bytes,
            });
        }
        Ok(())
    }
}
