use std::time::{Duration, Instant};
use std::{
    fs::File,
    io::Read,
    sync::{Arc, Mutex, MutexGuard},
};

use sha2::{Digest, Sha256};
use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, GetProcessTimes};

use crate::acp::redaction::{SafeDiagnostic, SafeStderrDiagnostic};

use super::artifact::VerifiedArtifact;
use super::artifact::process_image_path;
use super::io::StderrDrain;
use super::{observe_exit, try_observe_exit, wait_until};
use crate::acp::process::capacity::TreeCapacityLease;
use crate::acp::process::supervisor::{TreeExit, ZeroProcessToken};
use crate::acp::process::{ProcessIdentityReceipt, SpawnTimingReceipt};
use sage_process::{error::LaunchError, handle::NativeHandle, job::JobBoundary};

pub(crate) struct RunningTree {
    stdin: super::io::StdinEndpoint,
    stdout: super::io::StdoutEndpoint,
    control: TreeControl,
}

impl RunningTree {
    pub(crate) fn into_parts(
        self,
    ) -> (
        super::io::StdinEndpoint,
        super::io::StdoutEndpoint,
        TreeControl,
    ) {
        (self.stdin, self.stdout, self.control)
    }

    #[cfg(test)]
    pub(crate) fn control_for_test(&self) -> TreeControl {
        self.control.clone()
    }

    pub(super) fn new(
        stdin: super::io::StdinEndpoint,
        stdout: super::io::StdoutEndpoint,
        authority: TreeAuthority,
    ) -> Self {
        Self {
            stdin,
            stdout,
            control: TreeControl(Arc::new(authority)),
        }
    }
}

#[derive(Clone)]
pub(crate) struct TreeControl(Arc<TreeAuthority>);

impl TreeControl {
    pub(crate) fn log_wait_handle_observation(&self, checkpoint: &'static str) {
        match self.wait_handle_observation() {
            Ok(observation) => tracing::info!(
                checkpoint,
                wait_handle_pid = observation.process_id,
                wait_handle_image = ?observation.image,
                job_active_processes = observation.job_active_processes,
                job_total_processes = observation.job_total_processes,
                "ACP wait handle and job membership observed"
            ),
            Err(error) => tracing::warn!(checkpoint, ?error, "ACP wait-handle observation failed"),
        }
    }

    fn wait_handle_observation(&self) -> Result<WaitHandleObservation, LaunchError> {
        let process = self.0.process.raw();
        let process_id = unsafe { GetProcessId(process) };
        if process_id == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(WaitHandleObservation {
            process_id,
            image: process_image_path(process)?,
            job_active_processes: self.0.job.active_processes(self.0.active_process_limit)?,
            job_total_processes: self.0.job.total_processes()?,
        })
    }

    #[cfg(any(test, feature = "acp-fixture"))]
    pub(crate) fn spawn_timing_receipt(&self) -> SpawnTimingReceipt {
        self.0.spawn_timing
    }

    pub(crate) fn process_identity_receipt(&self) -> Result<ProcessIdentityReceipt, LaunchError> {
        let process = self.0.process.raw();
        let process_id = unsafe { GetProcessId(process) };
        let mut creation = windows_sys::Win32::Foundation::FILETIME::default();
        let mut exit = windows_sys::Win32::Foundation::FILETIME::default();
        let mut kernel = windows_sys::Win32::Foundation::FILETIME::default();
        let mut user = windows_sys::Win32::Foundation::FILETIME::default();
        // SAFETY: the supervisor retains query access to the live root handle,
        // and all FILETIME outputs are writable for the duration of the call.
        if process_id == 0
            || unsafe { GetProcessTimes(process, &mut creation, &mut exit, &mut kernel, &mut user) }
                == 0
        {
            return Err(LaunchError::ContainmentFailure);
        }
        let creation_time_filetime_100ns =
            (u64::from(creation.dwHighDateTime) << 32) | u64::from(creation.dwLowDateTime);
        if creation_time_filetime_100ns == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        // The retained root is node.exe for a verified npm .cmd launcher.
        // Never reuse the launcher digest here: diagnostics must describe the
        // actual process whose handle is still owned by the supervisor.
        let image = std::fs::canonicalize(process_image_path(process)?)
            .map_err(|_| LaunchError::IdentityMismatch)?;
        let (executable_sha256, executable_bytes) = bounded_image_digest(&image)?;
        Ok(ProcessIdentityReceipt::new(
            self.0.runtime_epoch,
            process_id,
            creation_time_filetime_100ns,
            executable_sha256,
            executable_bytes,
            image,
        ))
    }

    #[cfg(feature = "acp-fixture")]
    pub(crate) fn set_characterization_observation(
        &self,
        observation: crate::acp::characterization_observation::CharacterizationObservation,
    ) -> Result<(), LaunchError> {
        if *locked(&self.0.convergence)? {
            return Err(LaunchError::ContainmentFailure);
        }
        let mut state = locked(&self.0.characterization_observation)?;
        if state.job_zero_observed || state.sink.is_some() {
            return Err(LaunchError::ContainmentFailure);
        }
        state.sink = Some(observation);
        Ok(())
    }

    /// Dispatches termination without touching stdin/stdout or waiting for a
    /// blocked reader. Completion is observed separately through `wait`.
    pub(crate) fn request_termination(&self, reason: TreeExit) -> Result<(), LaunchError> {
        {
            locked(&self.0.requested_exit)?.get_or_insert(reason);
        }
        self.0.job.terminate()
    }

    pub(crate) fn wait_root_exit(&self) -> Result<TreeExit, LaunchError> {
        self.wait_root_exit_until(Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn observe_root_exit(&self) -> Result<TreeExit, LaunchError> {
        observe_exit(self.0.process.raw())?;
        self.observed_exit()
    }

    fn wait_root_exit_until(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        wait_until(self.0.process.raw(), deadline)?;
        self.observed_exit()
    }

    fn observed_exit(&self) -> Result<TreeExit, LaunchError> {
        let code = self.native_exit_code()?;
        let requested = *locked(&self.0.requested_exit)?;
        Ok(requested.unwrap_or(TreeExit::Exited { code: Some(code) }))
    }

    #[cfg(test)]
    pub(crate) fn wait(&self) -> Result<TreeExit, LaunchError> {
        self.wait_until(Instant::now() + self.0.terminate_deadline)
    }

    #[cfg(feature = "acp-fixture")]
    pub(crate) fn wait_before(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        self.wait_until(deadline)
    }

    #[cfg(test)]
    pub(crate) fn terminate(&self, reason: TreeExit) -> Result<TreeExit, LaunchError> {
        let deadline = Instant::now() + self.0.terminate_deadline;
        self.request_termination(reason)?;
        self.wait_until(deadline)
    }

    pub(crate) fn wait_zero(&self) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let exit = self.wait_until(Instant::now() + self.0.terminate_deadline)?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    pub(crate) fn wait_zero_before(
        &self,
        deadline: Instant,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let native_deadline = Instant::now() + self.0.terminate_deadline;
        let exit = self.wait_until(deadline.min(native_deadline))?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    #[cfg(test)]
    pub(crate) fn wait_zero_within(
        &self,
        timeout: Duration,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let exit = self.wait_until(Instant::now() + timeout.min(self.0.terminate_deadline))?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    #[cfg(test)]
    pub(crate) fn terminate_zero(
        &self,
        reason: TreeExit,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let deadline = Instant::now() + self.0.terminate_deadline;
        self.terminate_zero_before(reason, deadline)
    }

    pub(crate) fn terminate_zero_before(
        &self,
        reason: TreeExit,
        deadline: Instant,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let deadline = deadline.min(Instant::now() + self.0.terminate_deadline);
        self.request_termination(reason)?;
        let exit = self.wait_until(deadline)?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    pub(crate) fn stderr_summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        self.0.stderr.summary()
    }

    pub(crate) fn stderr_diagnostic(&self) -> Result<SafeStderrDiagnostic, LaunchError> {
        self.0.stderr.diagnostic()
    }

    /// Captures an already-observed native exit before any Sage-requested
    /// termination can replace its diagnostic classification.
    pub(crate) fn native_exit_code_if_exited(&self) -> Result<Option<i32>, LaunchError> {
        if !try_observe_exit(self.0.process.raw())? {
            return Ok(None);
        }
        self.native_exit_code().map(Some)
    }

    fn wait_until(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        let exit = self.wait_root_exit_until(deadline)?;
        self.0.converge_until(deadline)?;
        Ok(exit)
    }

    fn native_exit_code(&self) -> Result<i32, LaunchError> {
        let mut code = 0_u32;
        // SAFETY: the retained process handle is live and code is writable.
        if unsafe { GetExitCodeProcess(self.0.process.raw(), &mut code) } == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(code as i32)
    }

    #[cfg(test)]
    pub(crate) fn active_processes(&self) -> Result<u32, LaunchError> {
        self.0.job.active_processes(self.0.active_process_limit)
    }

    #[cfg(test)]
    pub(crate) fn total_processes(&self) -> Result<u32, LaunchError> {
        self.0.job.total_processes()
    }

    #[cfg(test)]
    pub(super) fn settled_root_and_stderr_membership_snapshot(&self) -> (u32, u32) {
        assert_eq!(try_observe_exit(self.0.process.raw()), Ok(true));
        assert_eq!(self.0.stderr.wait_until(Instant::now()), Ok(()));
        super::descendant_settlement_tests::membership_snapshot(self.0.job.raw())
    }

    #[cfg(test)]
    pub(super) fn contains_exact_handle(
        &self,
        process: windows_sys::Win32::Foundation::HANDLE,
    ) -> bool {
        let mut member = 0;
        // SAFETY: both handles are retained by the diagnostic test and supervisor.
        assert_ne!(
            unsafe {
                windows_sys::Win32::System::JobObjects::IsProcessInJob(
                    process,
                    self.0.job.raw(),
                    &mut member,
                )
            },
            0
        );
        member != 0
    }
}

const MAX_DIAGNOSTIC_IMAGE_BYTES: u64 = 512 * 1024 * 1024;

fn locked<T>(mutex: &Mutex<T>) -> Result<MutexGuard<'_, T>, LaunchError> {
    mutex.lock().map_err(|_| LaunchError::ContainmentFailure)
}

fn bounded_image_digest(path: &std::path::Path) -> Result<([u8; 32], u64), LaunchError> {
    let mut file = File::open(path).map_err(|_| LaunchError::IdentityMismatch)?;
    let bytes = file
        .metadata()
        .map_err(|_| LaunchError::IdentityMismatch)?
        .len();
    if bytes == 0 || bytes > MAX_DIAGNOSTIC_IMAGE_BYTES {
        return Err(LaunchError::IdentityMismatch);
    }
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut observed = 0_u64;
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|_| LaunchError::IdentityMismatch)?;
        if read == 0 {
            break;
        }
        observed = observed
            .saturating_add(u64::try_from(read).map_err(|_| LaunchError::IdentityMismatch)?);
        if observed > bytes {
            return Err(LaunchError::IdentityMismatch);
        }
        hasher.update(&buffer[..read]);
    }
    (observed == bytes)
        .then_some((hasher.finalize().into(), bytes))
        .ok_or(LaunchError::IdentityMismatch)
}

#[derive(Debug)]
pub(crate) struct WaitHandleObservation {
    pub(crate) process_id: u32,
    pub(crate) image: std::path::PathBuf,
    pub(crate) job_active_processes: u32,
    pub(crate) job_total_processes: u32,
}

pub(super) struct TreeAuthority {
    pub(super) process: NativeHandle,
    pub(super) thread: NativeHandle,
    pub(super) job: JobBoundary,
    pub(super) artifact: VerifiedArtifact,
    pub(super) stderr: StderrDrain,
    pub(super) lease: Mutex<Option<TreeCapacityLease>>,
    pub(super) active_process_limit: u32,
    pub(super) terminate_deadline: Duration,
    pub(super) absolute_cleanup_deadline: Option<Instant>,
    pub(super) requested_exit: Mutex<Option<TreeExit>>,
    pub(super) convergence: Mutex<bool>,
    #[cfg(feature = "acp-fixture")]
    pub(super) characterization_observation: Mutex<CharacterizationObservationState>,
    pub(super) runtime_epoch: u64,
    #[cfg_attr(
        not(feature = "acp-fixture"),
        allow(dead_code, reason = "only the fixture lane reads spawn timing so far")
    )]
    pub(super) spawn_timing: SpawnTimingReceipt,
}

#[cfg(feature = "acp-fixture")]
#[derive(Default)]
pub(super) struct CharacterizationObservationState {
    sink: Option<crate::acp::characterization_observation::CharacterizationObservation>,
    job_zero_observed: bool,
}

impl TreeAuthority {
    pub(super) fn converge_until(&self, deadline: Instant) -> Result<(), LaunchError> {
        let mut converged = locked(&self.convergence)?;
        if *converged {
            return Ok(());
        }
        wait_until(self.process.raw(), deadline)?;
        if self.job.active_processes(self.active_process_limit)? != 0 {
            self.job.terminate()?;
        }
        self.job
            .wait_for_zero(self.active_process_limit, deadline)?;
        #[cfg(feature = "acp-fixture")]
        {
            let observation = {
                let mut state = locked(&self.characterization_observation)?;
                if state.job_zero_observed {
                    None
                } else {
                    state.job_zero_observed = true;
                    state.sink.clone()
                }
            };
            if let Some(observation) = observation {
                observation.record(
                    crate::acp::characterization_observation::CharacterizationObservationEvent::JobZero,
                );
            }
        }
        self.stderr.wait_until(deadline)?;
        let lease = locked(&self.lease)?
            .take()
            .ok_or(LaunchError::ContainmentFailure)?;
        lease.release_after_zero()?;
        *converged = true;
        Ok(())
    }

    fn shutdown(&self) -> Result<(), LaunchError> {
        let native_deadline = Instant::now() + self.terminate_deadline;
        let deadline = self
            .absolute_cleanup_deadline
            .map_or(native_deadline, |absolute| native_deadline.min(absolute));
        if *locked(&self.convergence)? {
            return Ok(());
        }
        {
            locked(&self.requested_exit)?.get_or_insert(TreeExit::ParentShutdown);
        }
        self.job.terminate()?;
        self.converge_until(deadline)
    }
}

impl Drop for TreeAuthority {
    fn drop(&mut self) {
        let _ = self.shutdown();
        // Retain native handles and the verified executable until convergence.
        let _ = self.thread.raw();
        let _ = self.artifact.file.metadata();
    }
}
