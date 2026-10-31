use std::{
    process::Child,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

use crate::acp::{
    process::{ProcessIdentityReceipt, TreeCapacityLease},
    redaction::{SafeDiagnostic, SafeStderrDiagnostic, StderrRing},
};

use super::{
    super::supervisor::{LaunchError, TreeExit},
    spawn::ArtifactIdentity,
};

#[derive(Clone)]
pub(crate) struct TreeControl(pub(super) Arc<TreeAuthority>);

pub(super) struct TreeAuthority {
    pub(super) child: Mutex<Child>,
    pub(super) requested_exit: Mutex<Option<TreeExit>>,
    pub(super) lease: Mutex<Option<TreeCapacityLease>>,
    pub(super) stderr: Arc<Mutex<StderrRing>>,
    pub(super) stderr_done: Mutex<mpsc::Receiver<()>>,
    pub(super) converged: Mutex<bool>,
    pub(super) runtime_epoch: u64,
    pub(super) terminate_deadline: Duration,
    pub(super) artifact: ArtifactIdentity,
    pub(super) spawned_millis: u64,
}

impl TreeControl {
    pub(super) fn new(authority: TreeAuthority) -> Self {
        Self(Arc::new(authority))
    }

    pub(crate) fn log_wait_handle_observation(&self, checkpoint: &'static str) {
        tracing::info!(
            checkpoint,
            pid = self.pid(),
            "WSL Codex root-process watcher attached"
        );
    }

    pub(crate) fn process_identity_receipt(&self) -> Result<ProcessIdentityReceipt, LaunchError> {
        Ok(ProcessIdentityReceipt::new(
            self.0.runtime_epoch,
            self.pid(),
            self.0.spawned_millis.saturating_mul(10_000),
            self.0.artifact.sha256,
            self.0.artifact.byte_len,
            self.0.artifact.path.clone(),
        ))
    }

    pub(crate) fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        self.0
            .requested_exit
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .get_or_insert(reason);
        let mut child = self
            .0
            .child
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        if child.try_wait().map_err(|_| LaunchError::Io)?.is_none() {
            child.kill().map_err(|_| LaunchError::Io)?;
        }
        Ok(())
    }

    #[cfg(test)]
    pub(crate) fn terminate_zero(
        &self,
        reason: TreeExit,
    ) -> Result<(TreeExit, super::super::ZeroProcessToken), LaunchError> {
        self.terminate_zero_before(
            reason,
            std::time::Instant::now() + self.0.terminate_deadline,
        )
    }

    pub(crate) fn stderr_summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        self.0
            .stderr
            .lock()
            .map(|ring| ring.safe_summary())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(crate) fn stderr_diagnostic(&self) -> Result<SafeStderrDiagnostic, LaunchError> {
        self.0
            .stderr
            .lock()
            .map(|ring| ring.safe_diagnostic())
            .map_err(|_| LaunchError::ContainmentFailure)
    }

    pub(crate) fn native_exit_code_if_exited(&self) -> Result<Option<i32>, LaunchError> {
        self.0
            .child
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .try_wait()
            .map(|status| status.and_then(|value| value.code()))
            .map_err(|_| LaunchError::Io)
    }

    fn pid(&self) -> u32 {
        self.0
            .child
            .lock()
            .map(|child| child.id())
            .unwrap_or_default()
    }

    pub(super) fn requested_exit(&self) -> Option<TreeExit> {
        self.0.requested_exit.lock().ok().and_then(|exit| *exit)
    }
}

impl Drop for TreeAuthority {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut()
            && child.try_wait().ok().flatten().is_none()
        {
            let _ = child.kill();
        }
    }
}
