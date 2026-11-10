//! What Claude Code offers beyond Claude's ACP surface, and the native
//! requests answered by a control round trip: session controls and forks.

use serde_json::Value;

use super::{ClaudeSdkRuntime, MAX_OPEN_DELIVERIES, SdkBridge, sdk_frame};
use crate::acp::{
    bindings::adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
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

    /// A fork of this session through its latest reply, as Claude Code
    /// saved it (see `fork`).
    pub(super) fn fork_egress(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let config =
            super::super::fork::config_directory(&super::super::admission::launch_environment());
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
