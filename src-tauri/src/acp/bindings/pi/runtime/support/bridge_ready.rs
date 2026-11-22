use super::super::PiRuntime;
use crate::acp::bindings::{
    adapter::{AdapterBatch, AdapterRuntimeError},
    pi::bridge::install,
};
use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ready {
    version: u8,
    digest: String,
}

impl PiRuntime {
    pub(in crate::acp::bindings::pi::runtime) fn require_bridge_ready(
        &mut self,
    ) -> Result<(), AdapterRuntimeError> {
        if self.bridge.as_ref().is_some_and(|bridge| !bridge.ready) {
            self.poisoned = true;
            return Err(AdapterRuntimeError::ApprovalBridgeUnavailable);
        }
        Ok(())
    }

    pub(in crate::acp::bindings::pi::runtime) fn bridge_ready(
        &mut self,
        value: &serde_json::Value,
    ) -> Result<AdapterBatch, AdapterRuntimeError> {
        // session_shutdown precedes extension reload as well as process exit.
        // A previous runner's handshake cannot admit the replacement runner.
        if value
            .get("statusText")
            .is_none_or(serde_json::Value::is_null)
        {
            if let Some(bridge) = self.bridge.as_mut() {
                bridge.ready = false;
            }
            return AdapterBatch::new(Vec::new(), Vec::new());
        }
        let valid = value
            .get("statusText")
            .and_then(serde_json::Value::as_str)
            .filter(|body| body.len() <= 128)
            .and_then(|body| serde_json::from_str::<Ready>(body).ok())
            .is_some_and(|ready| ready.version == 1 && ready.digest == install::digest());
        if !valid {
            self.poisoned = true;
            return Err(AdapterRuntimeError::ApprovalBridgeUnavailable);
        }
        if let Some(bridge) = self.bridge.as_mut() {
            bridge.ready = true;
        }
        AdapterBatch::new(Vec::new(), Vec::new())
    }
}
