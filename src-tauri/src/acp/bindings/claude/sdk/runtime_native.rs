//! What Claude Code offers beyond Claude's ACP surface, and the native
//! requests answered by a control round trip: session controls and forks.

use serde_json::Value;

use super::{ClaudeSdkRuntime, MAX_OPEN_DELIVERIES, SdkBridge, sdk_frame};
use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
        claude::sdk::bridge::UsageReply,
    },
    goal::NativeRpcReply,
    intent::CapabilitySnapshotV1,
};

impl ClaudeSdkRuntime {
    /// A native request's control frame, recorded so an unwritten one is
    /// undone. One runs at a time, so its token finds it.
    pub(super) fn control_egress(
        &mut self,
        sdk: &Value,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sent = SdkBridge::sent(sdk).ok_or(AdapterRuntimeError::Frame)?;
        let frame = match sdk_frame(sdk) {
            Ok(frame) => frame,
            Err(error) => {
                self.bridge()?.undo(&sent);
                return Err(error);
            }
        };
        let token = EgressToken::protocol_notification();
        if self.sent.len() < MAX_OPEN_DELIVERIES {
            self.sent.push((token.clone(), sent));
        }
        Ok(AdapterEgress::new(frame, token))
    }

    /// The control frame for a session control Claude Code carries out.
    pub(super) fn control_request(
        &mut self,
        control: &crate::acp::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .session_control(control)
            .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
        self.control_egress(&sdk)
    }

    /// Claude Code's answer to a native request, until the host takes it,
    /// and what a usage read learned about the plan's limits.
    pub(super) fn answered(&mut self, reply: Option<NativeRpcReply>, usage: Option<UsageReply>) {
        if let Some(reply) = reply {
            self.native = Some(reply);
        }
        match usage {
            Some(UsageReply::Limits(snapshot)) => self.usage.replace(snapshot),
            Some(UsageReply::Unsupported) => self.usage.mark_unsupported(),
            None => {}
        }
    }

    /// The control frame that reads the plan's usage limits.
    pub(super) fn usage_request(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .usage_read()
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }

    /// Passive requests whose caller gave up (an MCP list or reconnect, or
    /// a usage read); their late replies are then dropped.
    pub(super) fn abandon_passive_requests(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.abandon_passive_requests();
        }
    }

    /// A fork of this session through its latest reply, as Claude Code
    /// saved it (see `fork`).
    pub(super) fn fork_egress(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let config = self.binding.config_directory();
        let sdk = self
            .bridge()?
            .fork(config)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }
}

/// What Claude Code does beyond Claude's ACP surface: it queues a message
/// sent while it works (see `bridge::steer`), compacts on its own
/// `/compact` prompt, which a Compact action sends as its Turn, and forks a
/// saved conversation when launched to (see `fork`). The
/// session's capabilities and its `Initialized` event must agree.
pub(super) fn sdk_capabilities(mut capabilities: CapabilitySnapshotV1) -> CapabilitySnapshotV1 {
    capabilities.active_turn_steer = capabilities.prompt;
    capabilities.native_session_compact = capabilities.prompt;
    capabilities.native_session_fork = capabilities.prompt;
    capabilities
}
