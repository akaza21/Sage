use super::*;
use crate::acp::{
    contract::SafeText,
    goal::{NativeRpcIntent, NativeRpcReply, NativeRpcRequest},
    runtime::HostError,
    scheduler::RuntimeOwner,
    session_control::SessionControl,
};

#[derive(Debug, thiserror::Error)]
pub(crate) enum SessionControlError {
    #[error("the harness declined the session control")]
    Refused(Option<SafeText<512>>),
    #[error("the session cannot take this control now")]
    Unavailable,
    #[error("the session control outcome is unknown")]
    RecoveryRequired,
}

impl AcpService {
    /// Carries out a session control on the owner's running session; the
    /// text says what the harness did, when it says.
    pub(crate) fn control_session(
        &self,
        owner: RuntimeOwner,
        control: SessionControl,
    ) -> Result<Option<SafeText<512>>, SessionControlError> {
        let lane = self
            .lane_for(&owner)
            .ok_or(SessionControlError::Unavailable)?;
        let host = lane
            .runtime
            .for_owner(&owner)
            .map_err(|_| SessionControlError::Unavailable)?;
        let result = host.native_rpc(NativeRpcIntent {
            owner: owner.clone(),
            request: NativeRpcRequest::Control(control),
        });
        if matches!(
            result,
            Err(HostError::Unavailable
                | HostError::Closed
                | HostError::Deadline
                | HostError::DeliveryUnknown
                | HostError::RecoveryRequired)
        ) {
            self.retain_config_effect_recovery(owner, host);
        }
        match result {
            Ok(NativeRpcReply::Controlled(summary)) => Ok(summary),
            Ok(NativeRpcReply::ControlRefused(reason)) => Err(SessionControlError::Refused(reason)),
            Ok(NativeRpcReply::Rejected) => Err(SessionControlError::Refused(None)),
            Ok(_) => Err(SessionControlError::RecoveryRequired),
            Err(HostError::Intent | HostError::State) => Err(SessionControlError::Unavailable),
            Err(_) => Err(SessionControlError::RecoveryRequired),
        }
    }
}
