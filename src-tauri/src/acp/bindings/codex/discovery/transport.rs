use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant},
};

use crate::acp::{
    bindings::{
        adapter::AdapterRuntimeError,
        codex::{AdmittedCodexBinding, CodexProjectRootAuthority, CodexRuntime},
    },
    contract::{RuntimeEpoch, SageAgentSessionId},
    limits::LIMITS,
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

enum ReaderEvent {
    Frame(Frame),
    Eof,
    Failed,
}

struct ReaderThread {
    events: mpsc::Receiver<ReaderEvent>,
    completion: mpsc::Receiver<ReaderCompletion>,
    overflowed: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReaderCompletion {
    Clean,
    Overflow,
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
    let mut protocol =
        CodexDiscoveryProtocol::new(binding.packet(), binding.binding_digest(), runtime_epoch);
    let mut transport =
        ProductionTransport::launch(binding, project_root, runtime_epoch, deadline, on_launch)?;
    let content =
        run_discovery_protocol(&mut protocol, &mut transport, runtime_epoch.get(), deadline)?;
    deadline.ensure_total()?;
    transport.revalidate(RevalidationCheckpoint::Publish)?;
    let (binding, project_root) = transport.into_authorities();
    Ok(AdmittedDiscovery {
        content,
        binding,
        project_root,
        runtime_epoch,
    })
}

struct ProductionTransport {
    stdin: StdinEndpoint,
    control: TreeControl,
    binding: Arc<AdmittedCodexBinding>,
    project_root: crate::project_registry::ProjectRootAuthority,
    reader: ReaderThread,
}

impl ProductionTransport {
    fn launch(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        deadline: CodexDiscoveryDeadline,
        on_launch: impl FnOnce(TreeControl) -> Result<(), CodexDiscoveryRunError>,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let session =
            SageAgentSessionId::parse(format!("session-codex-discovery-{}", runtime_epoch.get()))
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
            deadline
                .operation_remaining(Duration::from_millis(spec.deadlines.spawn_ms))?
                .as_millis(),
        )
        .unwrap_or(u64::MAX);
        spec.absolute_cleanup_deadline = Some(deadline.cleanup_end());
        let lease = TreeCapacityPool::global()
            .try_acquire(spec.limits.descendants)
            .map_err(map_launch)?;
        let tree = PlatformSupervisor::launch(&spec, lease)
            .map_err(|failure| settle_launch_failure(failure, runtime_epoch.get()))?;
        let project_root = spec.project_root;
        let (mut stdin, stdout, control) = tree.into_parts();
        if let Err(error) = deadline.ensure_operation() {
            stdin.close();
            return match control.terminate_zero_before(TreeExit::Deadline, deadline.cleanup_end()) {
                Ok((_, zero)) if zero.runtime_epoch() == runtime_epoch.get() => Err(error),
                _ => Err(CodexDiscoveryRunError::Cleanup),
            };
        }
        if let Err(error) = on_launch(control.clone()) {
            stdin.close();
            return match control
                .terminate_zero_before(TreeExit::ParentShutdown, deadline.cleanup_end())
            {
                Ok((_, zero)) if zero.runtime_epoch() == runtime_epoch.get() => Err(error),
                _ => Err(CodexDiscoveryRunError::Cleanup),
            };
        }
        let reader = match ReaderThread::spawn(stdout) {
            Ok(reader) => reader,
            Err(error) => {
                stdin.close();
                return match control
                    .terminate_zero_before(TreeExit::ProtocolFailure, deadline.cleanup_end())
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

    fn into_authorities(
        self,
    ) -> (
        Arc<AdmittedCodexBinding>,
        crate::project_registry::ProjectRootAuthority,
    ) {
        (self.binding, self.project_root)
    }
}

impl DiscoveryTransport for ProductionTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.stdin.write_frame(frame)
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        if self.reader.overflowed() {
            return Err(CodexDiscoveryRunError::Transport);
        }
        match self.reader.events.recv_timeout(timeout) {
            Ok(ReaderEvent::Frame(frame)) => Ok(Some(frame)),
            Ok(ReaderEvent::Eof) => Ok(None),
            Ok(ReaderEvent::Failed) | Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(CodexDiscoveryRunError::Transport)
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(CodexDiscoveryRunError::Timeout),
        }
    }

    fn revalidate(&self, _: RevalidationCheckpoint) -> Result<(), CodexDiscoveryRunError> {
        if self.reader.overflowed() {
            return Err(CodexDiscoveryRunError::Transport);
        }
        self.binding
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)?;
        self.project_root
            .revalidate()
            .map_err(|_| CodexDiscoveryRunError::Drift)
    }

    fn close_stdin(&mut self) {
        self.stdin.close();
    }

    fn terminate_zero(
        &mut self,
        protocol_failure: bool,
        deadline: CodexDiscoveryDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        let reason = if protocol_failure {
            TreeExit::ProtocolFailure
        } else {
            TreeExit::ParentShutdown
        };
        let (_, zero) = self
            .control
            .terminate_zero_before(reason, deadline.cleanup_end())
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        self.reader.finish_before(deadline.cleanup_end())?;
        Ok(DiscoveryZeroProcess::from_supervisor(zero))
    }
}

impl ReaderThread {
    fn spawn(
        mut stdout: crate::acp::process::StdoutEndpoint,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let (send_event, events) = mpsc::sync_channel(1);
        let (send_completion, completion) = mpsc::sync_channel(1);
        let overflowed = Arc::new(AtomicBool::new(false));
        let reader_overflowed = Arc::clone(&overflowed);
        let handle = thread::Builder::new()
            .name("sage-codex-discovery-stdout".to_owned())
            .spawn(move || {
                let completion = loop {
                    let event = match stdout.read_frame() {
                        Ok(Some(frame)) => ReaderEvent::Frame(frame),
                        Ok(None) => ReaderEvent::Eof,
                        Err(_) => ReaderEvent::Failed,
                    };
                    if let Some(completion) =
                        enqueue_reader_event(&send_event, event, &reader_overflowed)
                    {
                        break completion;
                    }
                };
                let _ = send_completion.send(completion);
            })
            .map_err(|_| CodexDiscoveryRunError::Transport)?;
        Ok(Self {
            events,
            completion,
            overflowed,
            handle: Some(handle),
        })
    }

    fn overflowed(&self) -> bool {
        self.overflowed.load(Ordering::Acquire)
    }

    fn finish_before(&mut self, deadline: Instant) -> Result<(), CodexDiscoveryRunError> {
        let timeout = deadline
            .saturating_duration_since(Instant::now())
            .min(Duration::from_millis(LIMITS.deadlines_ms.explicit_close));
        if timeout.is_zero() {
            return Err(CodexDiscoveryRunError::Cleanup);
        }
        let completion = self
            .completion
            .recv_timeout(timeout)
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        let Some(handle) = self.handle.take() else {
            return Err(CodexDiscoveryRunError::Cleanup);
        };
        handle.join().map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        match completion {
            ReaderCompletion::Clean => Ok(()),
            ReaderCompletion::Overflow => Err(CodexDiscoveryRunError::Transport),
        }
    }
}

fn enqueue_reader_event(
    sender: &mpsc::SyncSender<ReaderEvent>,
    event: ReaderEvent,
    overflowed: &AtomicBool,
) -> Option<ReaderCompletion> {
    let terminal = !matches!(&event, ReaderEvent::Frame(_));
    match sender.try_send(event) {
        Ok(()) if terminal => Some(ReaderCompletion::Clean),
        Ok(()) => None,
        Err(mpsc::TrySendError::Full(_)) => {
            overflowed.store(true, Ordering::Release);
            Some(ReaderCompletion::Overflow)
        }
        Err(mpsc::TrySendError::Disconnected(_)) => Some(ReaderCompletion::Clean),
    }
}

fn settle_launch_failure(failure: LaunchFailure, runtime_epoch: u64) -> CodexDiscoveryRunError {
    let launch_error = failure.error();
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
    } else {
        CodexDiscoveryRunError::Transport
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

    fn frame() -> Frame {
        let mut decoder = IncrementalFrameDecoder::new();
        decoder.push_chunk(b"{}\n").unwrap().unwrap()
    }
}
