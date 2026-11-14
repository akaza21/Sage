use std::time::{Duration, Instant};

use serde_json::Value;

use crate::acp::{
    process::ZeroProcessToken,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};

use super::{CodexDiscoveryContent, CodexDiscoveryError, CodexDiscoveryProtocol, ProtocolProgress};

const DISCOVERY_MODEL_PAGE_LIMIT: u32 = 4;
/// Reads allowed per expected response, so at most fifteen notifications may
/// precede it. The operation deadline still bounds the whole exchange.
const DISCOVERY_INTERLEAVED_NOTIFICATION_LIMIT: usize = 16;

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
        self.ensure_operation_at(Instant::now())
    }

    fn ensure_operation_at(self, now: Instant) -> Result<(), CodexDiscoveryRunError> {
        self.operation_remaining_at(now, Duration::MAX).map(drop)
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
    #[error("Codex discovery stdout framing failed")]
    Framing,
    #[error("Codex discovery stdout reader overflowed")]
    Overflow,
    #[error("Codex discovery stdout reader disconnected")]
    ReaderDisconnected,
    #[error("Codex discovery stdin write failed")]
    Write,
    #[error("Codex discovery process launch failed")]
    Launch,
    #[error("Codex discovery launch identity verification failed")]
    LaunchIdentity,
    #[error("Codex discovery is unavailable on this platform")]
    Unavailable,
    #[error("Codex discovery binding is not admitted")]
    BindingUnavailable,
    #[error("Codex discovery cannot project the admitted binding's capabilities")]
    CapabilityProjectionUnavailable,
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

    pub(crate) fn runtime_epoch(&self) -> u64 {
        self.runtime_epoch
    }

    #[cfg(any(test, all(windows, feature = "acp-fixture", debug_assertions)))]
    pub(crate) fn fixture(runtime_epoch: u64) -> Self {
        Self { runtime_epoch }
    }
}

pub(crate) trait DiscoveryTransport {
    fn now(&self) -> Instant {
        Instant::now()
    }

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
    deadline.ensure_operation_at(transport.now())?;
    let request = protocol.initialize_request()?;
    write(transport, request.into_frame(), deadline)?;
    let initialized = protocol.accept_initialize(&read(
        transport,
        deadline,
        crate::acp::limits::LIMITS.deadlines_ms.initialize,
    )?)?;
    write(transport, initialized, deadline)?;

    deadline.ensure_operation_at(transport.now())?;
    transport.revalidate(RevalidationCheckpoint::Account)?;
    let request = protocol.account_request()?;
    let account_id = request.id();
    write(transport, request.into_frame(), deadline)?;

    deadline.ensure_operation_at(transport.now())?;
    transport.revalidate(RevalidationCheckpoint::ModelPage)?;
    let request = protocol.model_request()?;
    let first_model_id = request.id();
    write(transport, request.into_frame(), deadline)?;
    let mut awaiting_account = true;
    let mut awaiting_first_model = true;
    let mut first_model_completed_before_account = false;
    while awaiting_account || awaiting_first_model {
        let frame = read(
            transport,
            deadline,
            crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
        )?;
        match response_id(&frame)? {
            id if id == account_id && awaiting_account => {
                protocol.accept_account(&frame)?;
                awaiting_account = false;
                if first_model_completed_before_account {
                    deadline.ensure_operation_at(transport.now())?;
                    transport.revalidate(RevalidationCheckpoint::Publish)?;
                    return protocol.take_completed_content().map_err(Into::into);
                }
            }
            id if id == first_model_id && awaiting_first_model => {
                match protocol.accept_model_page(&frame)? {
                    ProtocolProgress::More => {}
                    ProtocolProgress::AwaitingAccount => {
                        first_model_completed_before_account = true;
                    }
                    ProtocolProgress::Complete(content) => {
                        deadline.ensure_operation_at(transport.now())?;
                        transport.revalidate(RevalidationCheckpoint::Publish)?;
                        return Ok(content);
                    }
                }
                awaiting_first_model = false;
            }
            _ => return Err(CodexDiscoveryRunError::Protocol),
        }
    }

    loop {
        deadline.ensure_operation_at(transport.now())?;
        transport.revalidate(RevalidationCheckpoint::ModelPage)?;
        let request = protocol.model_request()?;
        write(transport, request.into_frame(), deadline)?;
        match protocol.accept_model_page(&read(
            transport,
            deadline,
            crate::acp::limits::LIMITS.deadlines_ms.ordinary_rpc,
        )?)? {
            ProtocolProgress::More => {}
            ProtocolProgress::AwaitingAccount => return Err(CodexDiscoveryRunError::Protocol),
            ProtocolProgress::Complete(content) => {
                deadline.ensure_operation_at(transport.now())?;
                transport.revalidate(RevalidationCheckpoint::Publish)?;
                return Ok(content);
            }
        }
    }
}

fn response_id(frame: &Frame) -> Result<u64, CodexDiscoveryRunError> {
    let response_id = serde_json::from_slice::<Value>(frame.as_bytes())
        .ok()
        .and_then(|value| value.get("id").and_then(Value::as_u64))
        .ok_or(CodexDiscoveryRunError::Protocol)?;
    tracing::info!(
        json_rpc_response_id = response_id,
        response_bytes = frame.as_bytes().len(),
        "Codex discovery response frame received"
    );
    Ok(response_id)
}

fn read(
    transport: &mut dyn DiscoveryTransport,
    deadline: CodexDiscoveryDeadline,
    timeout_ms: u64,
) -> Result<Frame, CodexDiscoveryRunError> {
    for _ in 0..DISCOVERY_INTERLEAVED_NOTIFICATION_LIMIT {
        let timeout =
            deadline.operation_remaining_at(transport.now(), Duration::from_millis(timeout_ms))?;
        let frame = transport
            .read_frame(timeout)?
            .ok_or(CodexDiscoveryRunError::Eof)?;
        if is_interleaved_notification(&frame) {
            continue;
        }
        return Ok(frame);
    }
    Err(CodexDiscoveryRunError::Protocol)
}

/// Discovery only needs responses, so it skips any server notification that
/// arrives in between. Codex emits several while it starts up (account and
/// remote-control status, `configWarning` for a stale config key), and newer
/// releases add more. A server request carries an `id` and is not skipped.
fn is_interleaved_notification(frame: &Frame) -> bool {
    let Ok(value) = serde_json::from_slice::<Value>(frame.as_bytes()) else {
        return false;
    };
    let Some(object) = value.as_object() else {
        return false;
    };
    let Some(method) = object.get("method").and_then(Value::as_str) else {
        return false;
    };
    let notification = object.keys().all(|key| {
        matches!(
            key.as_str(),
            "method" | "params" | "emittedAtMs" | "jsonrpc"
        )
    }) && object
        .get("params")
        .is_none_or(|params| params.is_object() || params.is_array())
        && object
            .get("emittedAtMs")
            .is_none_or(|emitted_at| emitted_at.as_u64().is_some());
    if !notification {
        return false;
    }
    if matches!(method, "account/updated" | "remoteControl/status/changed") {
        tracing::info!(
            json_rpc_method = method,
            json_rpc_params = ?object.get("params"),
            "Codex discovery ignored an interleaved Codex notification"
        );
    } else {
        // Other notifications may carry user configuration, so only their
        // method and size are recorded.
        tracing::info!(
            json_rpc_method = method,
            notification_bytes = frame.as_bytes().len(),
            "Codex discovery ignored an interleaved Codex notification"
        );
    }
    true
}

fn write(
    transport: &mut dyn DiscoveryTransport,
    frame: OutboundFrame,
    deadline: CodexDiscoveryDeadline,
) -> Result<(), CodexDiscoveryRunError> {
    deadline.ensure_operation_at(transport.now())?;
    log_request(&frame);
    match transport.write(&frame) {
        FrameWriteOutcome::Complete => Ok(()),
        FrameWriteOutcome::ZeroByteFailure | FrameWriteOutcome::DeliveryUnknown { .. } => {
            Err(CodexDiscoveryRunError::Write)
        }
    }
}

fn log_request(frame: &OutboundFrame) {
    let Ok(value) = serde_json::from_slice::<Value>(frame.payload()) else {
        tracing::warn!("Codex discovery refused to log a non-JSON outbound frame");
        return;
    };
    let Some(object) = value.as_object() else {
        tracing::warn!("Codex discovery refused to log a non-object outbound frame");
        return;
    };
    tracing::info!(
        json_rpc_id = ?object.get("id"),
        json_rpc_method = ?object.get("method"),
        json_rpc_params = ?object.get("params"),
        "Codex discovery request sent"
    );
}

#[cfg(test)]
mod tests;
