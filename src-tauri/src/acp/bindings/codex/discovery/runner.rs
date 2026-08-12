use std::time::{Duration, Instant};

use crate::acp::{
    process::ZeroProcessToken,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};

use super::{CodexDiscoveryContent, CodexDiscoveryError, CodexDiscoveryProtocol, ProtocolProgress};

const DISCOVERY_MODEL_PAGE_LIMIT: u32 = 4;

#[derive(Debug, Clone, Copy)]
pub(crate) struct CodexDiscoveryDeadline {
    operation_end: Instant,
    cleanup_end: Instant,
}

impl CodexDiscoveryDeadline {
    pub(crate) fn new() -> Result<Self, CodexDiscoveryRunError> {
        let limits = crate::acp::limits::LIMITS.deadlines_ms;
        let operation_ms = limits
            .spawn
            .saturating_add(limits.initialize)
            .saturating_add(
                limits
                    .ordinary_rpc
                    .saturating_mul(1 + u64::from(DISCOVERY_MODEL_PAGE_LIMIT)),
            );
        let cleanup_ms = limits
            .tree_termination
            .saturating_add(limits.explicit_close);
        Self::from_budgets(
            Instant::now(),
            Duration::from_millis(operation_ms),
            Duration::from_millis(cleanup_ms),
        )
    }

    fn from_budgets(
        admitted: Instant,
        operation: Duration,
        cleanup: Duration,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let operation_end = admitted
            .checked_add(operation)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        let cleanup_end = operation_end
            .checked_add(cleanup)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        Ok(Self {
            operation_end,
            cleanup_end,
        })
    }

    pub(crate) fn operation_remaining(
        self,
        phase_cap: Duration,
    ) -> Result<Duration, CodexDiscoveryRunError> {
        self.operation_remaining_at(Instant::now(), phase_cap)
    }

    fn operation_remaining_at(
        self,
        now: Instant,
        phase_cap: Duration,
    ) -> Result<Duration, CodexDiscoveryRunError> {
        let remaining = self
            .operation_end
            .saturating_duration_since(now)
            .min(phase_cap);
        (!remaining.is_zero())
            .then_some(remaining)
            .ok_or(CodexDiscoveryRunError::Timeout)
    }

    pub(crate) fn ensure_operation(self) -> Result<(), CodexDiscoveryRunError> {
        self.operation_remaining(Duration::MAX).map(drop)
    }

    pub(crate) fn ensure_total(self) -> Result<(), CodexDiscoveryRunError> {
        (!self
            .cleanup_end
            .saturating_duration_since(Instant::now())
            .is_zero())
        .then_some(())
        .ok_or(CodexDiscoveryRunError::Timeout)
    }

    pub(crate) fn cleanup_end(self) -> Instant {
        self.cleanup_end
    }

    pub(crate) fn operation_end(self) -> Instant {
        self.operation_end
    }

    #[cfg(test)]
    pub(crate) fn for_test(operation: Duration, cleanup: Duration) -> Self {
        Self::from_budgets(Instant::now(), operation, cleanup).expect("bounded test deadline")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RevalidationCheckpoint {
    Account,
    ModelPage,
    Publish,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexDiscoveryRunError {
    #[error("Codex discovery protocol failed")]
    Protocol,
    #[error("Codex discovery timed out")]
    Timeout,
    #[error("Codex discovery transport ended")]
    Eof,
    #[error("Codex discovery transport failed")]
    Transport,
    #[error("Codex discovery is unavailable on this platform")]
    Unavailable,
    #[error("Codex discovery binding is not admitted")]
    BindingUnavailable,
    #[error("Codex discovery authority drifted")]
    Drift,
    #[error("Codex discovery scope is invalid")]
    InvalidScope,
    #[error("Codex discovery is already running")]
    Busy,
    #[error("Codex discovery process cleanup could not prove zero processes")]
    Cleanup,
}

impl From<CodexDiscoveryError> for CodexDiscoveryRunError {
    fn from(_: CodexDiscoveryError) -> Self {
        Self::Protocol
    }
}

pub(crate) struct DiscoveryZeroProcess {
    runtime_epoch: u64,
}

impl DiscoveryZeroProcess {
    pub(crate) fn from_supervisor(token: ZeroProcessToken) -> Self {
        Self {
            runtime_epoch: token.runtime_epoch(),
        }
    }

    #[cfg(test)]
    pub(crate) fn fixture(runtime_epoch: u64) -> Self {
        Self { runtime_epoch }
    }
}

pub(crate) trait DiscoveryTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome;
    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError>;
    fn revalidate(&self, checkpoint: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError>;
    fn close_stdin(&mut self);
    fn terminate_zero(
        &mut self,
        protocol_failure: bool,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError>;
}

pub(crate) fn run_discovery_protocol(
    protocol: &mut CodexDiscoveryProtocol,
    transport: &mut dyn DiscoveryTransport,
    runtime_epoch: u64,
    deadline: CodexDiscoveryDeadline,
) -> Result<CodexDiscoveryContent, CodexDiscoveryRunError> {
    let result = execute(protocol, transport, deadline);
    transport.close_stdin();
    let zero = transport.terminate_zero(result.is_err(), deadline);
    match (result, zero) {
        (Ok(content), Ok(zero)) if zero.runtime_epoch == runtime_epoch => Ok(content),
        (Err(error), Ok(zero)) if zero.runtime_epoch == runtime_epoch => Err(error),
        (Ok(_), Err(error)) => Err(error),
        (Err(_), Err(CodexDiscoveryRunError::Cleanup)) => Err(CodexDiscoveryRunError::Cleanup),
        (Err(error), Err(_)) => Err(error),
        _ => Err(CodexDiscoveryRunError::Cleanup),
    }
}

fn execute(
    protocol: &mut CodexDiscoveryProtocol,
    transport: &mut dyn DiscoveryTransport,
    deadline: CodexDiscoveryDeadline,
) -> Result<CodexDiscoveryContent, CodexDiscoveryRunError> {
    deadline.ensure_operation()?;
    let request = protocol.initialize_request()?;
    write(transport, request.into_frame(), deadline)?;
    let initialized = protocol.accept_initialize(&read(
        transport,
        deadline,
        crate::acp::limits::LIMITS.deadlines_ms.initialize,
    )?)?;
    write(transport, initialized, deadline)?;

    deadline.ensure_operation()?;
    transport.revalidate(RevalidationCheckpoint::Account)?;
    let request = protocol.account_request()?;
    write(transport, request.into_frame(), deadline)?;
    protocol.accept_account(&read(
        transport,
        deadline,
        crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
    )?)?;

    loop {
        deadline.ensure_operation()?;
        transport.revalidate(RevalidationCheckpoint::ModelPage)?;
        let request = protocol.model_request()?;
        write(transport, request.into_frame(), deadline)?;
        match protocol.accept_model_page(&read(
            transport,
            deadline,
            crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
        )?)? {
            ProtocolProgress::More => {}
            ProtocolProgress::Complete(content) => {
                deadline.ensure_operation()?;
                transport.revalidate(RevalidationCheckpoint::Publish)?;
                return Ok(content);
            }
        }
    }
}

fn read(
    transport: &mut dyn DiscoveryTransport,
    deadline: CodexDiscoveryDeadline,
    timeout_ms: u64,
) -> Result<Frame, CodexDiscoveryRunError> {
    let timeout = deadline.operation_remaining(Duration::from_millis(timeout_ms))?;
    transport
        .read_frame(timeout)?
        .ok_or(CodexDiscoveryRunError::Eof)
}

fn write(
    transport: &mut dyn DiscoveryTransport,
    frame: OutboundFrame,
    deadline: CodexDiscoveryDeadline,
) -> Result<(), CodexDiscoveryRunError> {
    deadline.ensure_operation()?;
    match transport.write(&frame) {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure | FrameWriteOutcome::DeliveryUnknown { .. } => {
            Err(CodexDiscoveryRunError::Transport)
        }
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
