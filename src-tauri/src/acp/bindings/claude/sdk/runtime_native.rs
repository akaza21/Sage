use serde_json::Value;

use super::{ClaudeSdkRuntime, MAX_OPEN_DELIVERIES, SdkBridge, sdk_frame};
use crate::acp::{
    bindings::{
        adapter::{AdapterEgress, AdapterRuntimeError, EgressToken},
        claude::sdk::bridge::UsageReply,
    },
    goal::NativeRpcReply,
};
use sage_acp_protocol::intent::CapabilitySnapshotV1;

impl ClaudeSdkRuntime {
    /// Only one native control request runs at a time, so its delivery token identifies it.
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

    pub(super) fn control_request(
        &mut self,
        control: &sage_acp_protocol::session_control::SessionControl,
    ) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .session_control(control)
            .map_err(|_| AdapterRuntimeError::UnsupportedCommand)?;
        self.control_egress(&sdk)
    }

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

    pub(super) fn usage_request(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let sdk = self
            .bridge()?
            .usage_read()
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }

    pub(super) fn abandon_passive_requests(&mut self) {
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.abandon_passive_requests();
        }
    }

    pub(super) fn fork_egress(&mut self) -> Result<AdapterEgress, AdapterRuntimeError> {
        let config = self.binding.config_directory();
        let sdk = self
            .bridge()?
            .fork(config)
            .map_err(|_| AdapterRuntimeError::Operation)?;
        self.control_egress(&sdk)
    }
}

/// The session's capabilities and its `Initialized` event must agree.
pub(super) fn sdk_capabilities(mut capabilities: CapabilitySnapshotV1) -> CapabilitySnapshotV1 {
    capabilities.active_turn_steer = capabilities.prompt;
    capabilities.native_session_compact = capabilities.prompt;
    capabilities.native_session_fork = capabilities.prompt;
    capabilities
}
