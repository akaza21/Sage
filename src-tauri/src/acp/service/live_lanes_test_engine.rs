use super::*;
use crate::acp::{runtime::*, store::AdapterTurnCursor};
use sage_acp_protocol::contract::SageTurnId;
use std::time::Instant;

/// With `answering`, a native RPC waits there for the test to answer it.
#[derive(Default)]
pub(super) struct IdleEngine {
    pub(super) answering: Option<Arc<Mutex<Option<NativeRpcCompletionSink>>>>,
}

impl RuntimeEngine for IdleEngine {
    fn activate(&mut self, _: &RuntimeOwner) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_native_rpc(
        &mut self,
        _: &crate::acp::goal::NativeRpcIntent,
        _: Instant,
        completion: NativeRpcCompletionSink,
    ) -> Result<(), HostError> {
        match &self.answering {
            Some(pending) => {
                *pending.lock().unwrap() = Some(completion);
            }
            None => completion.complete(Err(HostError::State)),
        }
        Ok(())
    }
    fn prepare_prompt(&mut self, _: &HostPromptIntent) -> Result<SageTurnId, HostError> {
        Err(HostError::State)
    }
    fn dispatch_prompt(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn retry_prompt(&mut self, _: &RuntimeOwner, _: &SageTurnId) -> Result<PromptWrite, HostError> {
        Err(HostError::State)
    }
    fn record_delivery_unknown(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn record_retry_exhausted(
        &mut self,
        _: &RuntimeOwner,
        _: &SageTurnId,
    ) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn begin_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn dispatch_cancel(&mut self, _: &HostCancelIntent) -> Result<(), HostError> {
        Err(HostError::State)
    }
    fn accept_terminal(&mut self, _: &AdapterTurnCursor) -> Result<(), HostError> {
        Ok(())
    }
    fn begin_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn converge_zero(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn finalize_close(&mut self, _: &HostCloseIntent) -> Result<(), HostError> {
        Ok(())
    }
    fn shutdown(&mut self, _: Instant) -> Result<(), HostError> {
        Ok(())
    }
}

pub(super) struct Shutdown;

impl RuntimeShutdownRecovery for Shutdown {
    fn shutdown_recovery_before(
        &self,
        _: Instant,
    ) -> Result<crate::acp::process::ZeroProcessToken, HostError> {
        Ok(crate::acp::process::ZeroProcessToken::for_test(1))
    }
}
