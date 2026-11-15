use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::acp::{
    bindings::codex::{
        AdmittedCodexBinding, AuthTransport, CodexAuthDeadline, CodexDiscoveryRunError,
        CodexProcessTransport, CodexProjectRootAuthority, DiscoveryZeroProcess,
        ProcessLaunchPolicy,
    },
    process::TreeControl,
    transport::{Frame, FrameWriteOutcome, OutboundFrame},
};
use sage_acp_protocol::contract::RuntimeEpoch;

pub(crate) struct ProductionAuthTransport {
    process: CodexProcessTransport,
    cancel: Arc<AtomicBool>,
}

impl ProductionAuthTransport {
    pub(crate) fn launch(
        binding: Arc<AdmittedCodexBinding>,
        project_root: CodexProjectRootAuthority,
        runtime_epoch: RuntimeEpoch,
        deadline: CodexAuthDeadline,
        cancel: Arc<AtomicBool>,
        on_launch: impl FnOnce(TreeControl) -> Result<(), CodexDiscoveryRunError>,
    ) -> Result<Self, CodexDiscoveryRunError> {
        let spawn = deadline
            .operation_end()
            .saturating_duration_since(std::time::Instant::now());
        let process = CodexProcessTransport::launch(
            binding,
            project_root,
            runtime_epoch,
            ProcessLaunchPolicy {
                session_kind: "auth",
                spawn_timeout: spawn,
                cleanup_end: deadline.cleanup_end(),
                reader_slots: 4,
            },
            on_launch,
        )?;
        Ok(Self { process, cancel })
    }

    pub(crate) fn revalidate(&self) -> Result<(), CodexDiscoveryRunError> {
        self.process.revalidate()
    }
}

impl AuthTransport for ProductionAuthTransport {
    fn write(&mut self, frame: &OutboundFrame) -> FrameWriteOutcome {
        self.process.write(frame)
    }

    fn read_frame(&mut self, timeout: Duration) -> Result<Option<Frame>, CodexDiscoveryRunError> {
        self.process.read_frame(timeout)
    }

    fn user_cancel_requested(&self) -> bool {
        self.cancel.load(Ordering::Acquire)
    }

    fn close_stdin(&mut self) {
        self.process.close_stdin();
    }

    fn terminate_zero(
        &mut self,
        failed: bool,
        deadline: CodexAuthDeadline,
    ) -> Result<DiscoveryZeroProcess, CodexDiscoveryRunError> {
        self.process
            .terminate_zero_before(failed, deadline.cleanup_end())
    }
}
