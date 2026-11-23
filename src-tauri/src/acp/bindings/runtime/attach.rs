use super::*;
use sage_acp_protocol::scheduler::RuntimeOwner;
use std::sync::Arc;

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
