use super::{AdapterRuntimeError, NativeRuntime};
use sage_acp_protocol::scheduler::RuntimeOwner;
use std::sync::Arc;

impl NativeRuntime {
    pub(super) fn publish_config(&self) -> Result<(), AdapterRuntimeError> {
        self.publish_commands();
        if let Some((owner, channel)) = &self.1 {
            match self.0.session_config() {
                Some(config) => channel.update(owner, config),
                None => channel.remove(owner),
            }
            .map_err(|_| AdapterRuntimeError::Frame)?;
            channel.set_hydrating(owner, self.0.config_hydrating());
        }
        Ok(())
    }

    pub(super) fn publish_goal(&self) {
        if let Some((owner, channel)) = &self.2
            && let Some(goal) = self.0.goal_state()
        {
            channel.update_observed(owner, goal);
        }
    }
}

impl Drop for NativeRuntime {
    fn drop(&mut self) {
        if let Some((owner, channel)) = &self.3 {
            channel.clear(owner);
        }
        if let Some((owner, channel)) = &self.4 {
            channel.clear(owner);
        }
    }
}

/// The lane channels and services a runtime publishes to once it is
/// assembled for one owner.
impl NativeRuntime {
    pub(crate) fn attach_titles(
        &mut self,
        titles: crate::acp::service::chat_title::TitleScheduler,
    ) {
        self.5 = Some(titles);
    }

    /// Lends the binding's usage cell to the lane once; a binding without
    /// one leaves the lane empty, which reads as unsupported.
    pub(crate) fn attach_usage(
        &mut self,
        owner: RuntimeOwner,
        channel: Arc<sage_acp_protocol::usage::UsageChannel>,
    ) {
        if let Some(cell) = self.0.usage() {
            channel.attach(owner.clone(), cell);
            self.4 = Some((owner, channel));
        }
    }

    pub(crate) fn attach_config(
        &mut self,
        owner: RuntimeOwner,
        channel: Arc<crate::acp::session_config::ConfigChannel>,
    ) {
        self.1 = Some((owner, channel));
    }

    pub(crate) fn attach_goal(
        &mut self,
        owner: RuntimeOwner,
        channel: Arc<crate::acp::goal::GoalChannel>,
    ) {
        self.2 = Some((owner, channel));
    }

    pub(crate) fn attach_commands(
        &mut self,
        owner: RuntimeOwner,
        channel: Arc<crate::acp::slash_commands::CommandChannel>,
    ) {
        self.3 = Some((owner, channel));
    }

    pub(crate) fn attach_catalog_capabilities(
        &mut self,
        capabilities: crate::acp::service::catalog_capabilities::CatalogCapabilities,
    ) {
        self.6 = Some(capabilities);
        // A harness already initialized reports its negotiated set now.
        self.follow_catalog_capabilities(None);
    }
}
