use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{AdmittedCodexBinding, CodexProjectRootAuthority, CodexRuntime},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    process::{
        LaunchError, LaunchFailure, PlatformSupervisor, StdinEndpoint, TreeCapacityPool,
        TreeControl, TreeExit,
    },
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};

use super::{
    CodexDiscoveryContent, CodexDiscoveryProtocol,
    runner::{
        CodexDiscoveryDeadline, CodexDiscoveryRunError, DiscoveryTransport, DiscoveryZeroProcess,
        RevalidationCheckpoint, run_discovery_protocol,
    },
};

mod reader;
#[cfg(test)]
use reader::{ReaderCompletion, enqueue_reader_event};
use reader::{ReaderEvent, ReaderThread};

// Codex may emit initialized and capability notifications immediately after
// the initialize response. Keep a bounded burst buffer so that valid output
// cannot race the synchronous discovery protocol into a false transport error.
const DISCOVERY_READER_SLOTS: usize = 4;

pub(crate) struct ProcessLaunchPolicy {
    pub(crate) session_kind: &'static str,
    pub(crate) spawn_timeout: Duration,
    pub(crate) cleanup_end: Instant,
    pub(crate) reader_slots: usize,
}

pub(crate) struct AdmittedDiscovery {
    pub(crate) content: CodexDiscoveryContent,
    binding: Arc<AdmittedCodexBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    runtime_epoch: RuntimeEpoch,
}

impl AdmittedDiscovery {
    pub(crate) fn revalidate(&self) -> Result<(), CodexDiscoveryRunError> {
        self.binding
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        self.project_root
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)
    }

    pub(crate) fn runtime_epoch(&self) -> RuntimeEpoch {
        self.runtime_epoch
    }
}

pub(crate) fn discover_admitted(
    binding: Arc<AdmittedCodexBinding>,
    project_root: CodexProjectRootAuthority,
    runtime_epoch: RuntimeEpoch,
    deadline: CodexDiscoveryDeadline,
    on_launch: impl FnOnce(TreeControl) -> Result<(), CodexDiscoveryRunError>,
) -> Result<AdmittedDiscovery, CodexDiscoveryRunError> {
    let mut protocol = CodexDiscoveryProtocol::from_admitted_binding(&binding, runtime_epoch);
    let spawn_timeout = deadline.operation_remaining(Duration::from_millis(
        crate::acp::limits::LIMITS.deadlines_ms.spawn,
    ))?;
    let mut transport = CodexProcessTransport::launch(
        binding,
        project_root,
        runtime_epoch,
        ProcessLaunchPolicy {
            session_kind: "discovery",
            spawn_timeout,
            cleanup_end: deadline.cleanup_end(),
            reader_slots: DISCOVERY_READER_SLOTS,
        },
        on_launch,
    )?;
    let content =
        run_discovery_protocol(&mut protocol, &mut transport, runtime_epoch.get(), deadline)?;
    deadline.ensure_total()?;
    transport.revalidate()?;
    let (binding, project_root) = transport.into_authorities();
    Ok(AdmittedDiscovery {
        content,
        binding,
        project_root,
        runtime_epoch,
    })
}

pub(crate) struct CodexProcessTransport {
    stdin: StdinEndpoint,
    control: TreeControl,
    binding: Arc<AdmittedCodexBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    reader: ReaderThread,
}

impl CodexProcessTransport {
    pub(crate) fn launch(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        policy: ProcessLaunchPolicy,
        on_launch: impl FnOnce(TreeControl) -> Result<(), CodexDiscoveryRunError>,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let session = SageAgentSessionId::parse(format!(
            "session-codex-{}-{}",
            policy.session_kind,
            runtime_epoch.get()
        ))
        .map_err(|_| CodexDiscoveryRunError::Transport)?;
        let mut runtime = CodexRuntime::from_admitted_binding(
            Arc::clone(&binding),
            project_root,
            runtime_epoch,
            session,
        )
        .map_err(map_runtime)?;
        let mut spec = runtime.launch_spec().map_err(map_runtime)?;
        spec.deadlines.spawn_ms = u64::try_from(
            policy
                .spawn_timeout
                .min(Duration::from_millis(spec.deadlines.spawn_ms))
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        spec.absolute_cleanup_deadline = Some(policy.cleanup_end);
        let lease = TreeCapacityPool::global()
            .try_acquire(spec.limits.descendants)
            .map_err(map_launch)?;
        let tree = PlatformSupervisor::launch(&spec, lease)
            .map_err(|failure| settle_launch_failure(failure, runtime_epoch.get()))?;
        let project_root = spec.project_root;
        let (mut stdin, stdout, control) = tree.into_parts();
        if policy.spawn_timeout.is_zero() {
            stdin.close();
            return match control.terminate_zero_before(TreeExit::Deadline, policy.cleanup_end) {
                Ok((_, zero)) if zero.runtime_epoch() == runtime_epoch.get() => {
                    Err(CodexDiscoveryRunError::Timeout)
                }
                _ => Err(CodexDiscoveryRunError::Cleanup),
            };
        }
        if let Err(error) = on_launch(control.clone()) {
            stdin.close();
            return match control.terminate_zero_before(TreeExit::ParentShutdown, policy.cleanup_end)
            {
                Ok((_, zero)) if zero.runtime_epoch() == runtime_epoch.get() => Err(error),
                _ => Err(CodexDiscoveryRunError::Cleanup),
            };
        }
        let reader = match ReaderThread::spawn(stdout, policy.reader_slots, policy.session_kind) {
            Ok(reader) => reader,
            Err(error) => {
                stdin.close();
                return match control
                    .terminate_zero_before(TreeExit::ProtocolFailure, policy.cleanup_end)
                {
                    Ok((_, zero)) if zero.runtime_epoch() == runtime_epoch.get() => Err(error),
                    _ => Err(CodexDiscoveryRunError::Cleanup),
                };
            }
        };
        Ok(Self {
            stdin,
            control,
            binding,
            project_root,
            reader,
        })
    }

    pub(crate) fn into_authorities(
        self,
    ) -> (
        Arc<AdmittedCodexBinding>,
        crate::project_registry::ProjectRootAuthority,
    ) {
        (self.binding, self.project_root)
    }

    pub(crate) fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        let outcome = self.stdin.write_frame(frame);
        if !matches!(outcome, FrameWriteOutcome::Complete) {
            tracing::warn!(
                ?outcome,
                "Codex discovery transport classified stdin write failure"
            );
        }
        outcome
    }

    pub(crate) fn read_frame(
        &mut self,
        timeout: Duration,
    ) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        if self.reader.overflowed() {
            return Err(CodexDiscoveryRunError::Overflow);
        }
        match self.reader.events.recv_timeout(timeout) {
            Ok(ReaderEvent::Frame(frame)) => Ok(Some(frame)),
            Ok(ReaderEvent::Eof) => {
                tracing::warn!("Codex discovery transport classified stdout EOF");
                Ok(None)
            }
            Ok(ReaderEvent::FramingFailed(error)) => {
                tracing::warn!(
                    ?error,
                    "Codex discovery transport classified stdout framing failure"
                );
                Err(CodexDiscoveryRunError::Framing)
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                tracing::warn!("Codex discovery transport classified stdout reader disconnection");
                Err(CodexDiscoveryRunError::ReaderDisconnected)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {
                tracing::warn!("Codex discovery transport classified read timeout");
                Err(CodexDiscoveryRunError::Timeout)
            }
        }
    }

    pub(crate) fn revalidate(&self) -> Result<(), CodexDiscoveryRunError> {
        if self.reader.overflowed() {
            return Err(CodexDiscoveryRunError::Overflow);
        }
        self.binding
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        self.project_root
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)
    }

    pub(crate) fn close_stdin(&mut self) {
        self.stdin.close();
    }

    pub(crate) fn terminate_zero_before(
        &mut self,
        failed: bool,
        cleanup_end: Instant,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        let reason = if failed {
            TreeExit::ProtocolFailure
        } else {
            TreeExit::ParentShutdown
        };
        let (_, zero) = self
            .control
            .terminate_zero_before(reason, cleanup_end)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        self.reader.finish_before(cleanup_end)?;
        Ok(DiscoveryZeroProcess::from_supervisor(zero))
    }
}

impl DiscoveryTransport for CodexProcessTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.write(frame)
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        self.read_frame(timeout)
    }

    fn revalidate(&self, _: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
        self.revalidate()
    }

    fn close_stdin(&mut self) {
        self.close_stdin();
    }

    fn terminate_zero(
        &mut self,
        protocol_failure: bool,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        self.terminate_zero_before(protocol_failure, deadline.cleanup_end())
    }
}

fn settle_launch_failure(failure: LaunchFailure, runtime_epoch: u64) -> CodexDiscoveryRunError {
    let launch_error = failure.error();
    tracing::warn!(
        ?launch_error,
        runtime_epoch,
        "Codex discovery process launch failed"
    );
    let token = match failure.into_zero_process() {
        Ok(token) => Ok(token),
        Err(failure) => match failure.into_recovery() {
            Ok((_, recovery)) => recovery
                .retry_zero()
                .map_err(|_| CodexDiscoveryRunError::Cleanup),
            Err(_) => Err(CodexDiscoveryRunError::Cleanup),
        },
    };
    match token {
        Ok(token) if token.runtime_epoch() == runtime_epoch => map_launch(launch_error),
        _ => CodexDiscoveryRunError::Cleanup,
    }
}

fn map_runtime(_: AdapterRuntimeError) -> CodexDiscoveryRunError {
    CodexDiscoveryRunError::Drift
}

fn map_launch(error: LaunchError) -> CodexDiscoveryRunError {
    if error == LaunchError::PlatformUnavailable {
        CodexDiscoveryRunError::Unavailable
    } else if error == LaunchError::IdentityMismatch {
        tracing::warn!(
            ?error,
            "Codex discovery transport classified launch identity mismatch"
        );
        CodexDiscoveryRunError::LaunchIdentity
    } else {
        tracing::warn!(
            ?error,
            "Codex discovery transport classified process launch failure"
        );
        CodexDiscoveryRunError::Launch
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::transport::IncrementalFrameDecoder;

    #[test]
    fn full_reader_channel_is_an_explicit_overflow_not_clean_eof() {
        let (sender, _receiver) = mpsc::sync_channel(1);
        let overflowed = AtomicBool::new(false);
        assert_eq!(
            enqueue_reader_event(&sender, ReaderEvent::Frame(frame()), &overflowed),
            None
        );
        assert_eq!(
            enqueue_reader_event(&sender, ReaderEvent::Frame(frame()), &overflowed),
            Some(ReaderCompletion::Overflow)
        );
        assert!(overflowed.load(Ordering::Acquire));
    }

    #[test]
    fn discovery_reader_keeps_the_initialize_response_and_follow_up_notifications() {
        let (sender, _receiver) = mpsc::sync_channel(DISCOVERY_READER_SLOTS);
        let overflowed = AtomicBool::new(false);

        for _ in 0..DISCOVERY_READER_SLOTS {
            assert_eq!(
                enqueue_reader_event(&sender, ReaderEvent::Frame(frame()), &overflowed),
                None
            );
        }

        assert!(!overflowed.load(Ordering::Acquire));
        assert_eq!(DISCOVERY_READER_SLOTS, 4);
    }

    fn frame() -> Frame {
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.push_chunk(b"{}\n").unwrap().unwrap()
    }
}
