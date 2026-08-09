use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::{GetExitCodeProcess, GetProcessId, GetProcessTimes};

use crate::acp::redaction::SafeDiagnostic;

use super::artifact::VerifiedArtifact;
use super::io::StderrDrain;
use super::job::JobBoundary;
use super::native::NativeHandle;
use super::{observe_exit, wait_until};
use crate::acp::process::capacity::TreeCapacityLease;
use crate::acp::process::supervisor::{LaunchError, TreeExit, ZeroProcessToken};
use crate::acp::process::{ProcessIdentityReceipt, SpawnTimingReceipt};

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
        Ok(ProcessIdentityReceipt::new(
            self.0.runtime_epoch,
            process_id,
            creation_time_filetime_100ns,
            self.0.artifact.sha256(),
            self.0.artifact.byte_len(),
        ))
    }

    #[cfg(feature = "acp-fixture")]
    pub(crate) fn set_characterization_observation(
        &self,
        observation: crate::acp::characterization_observation::CharacterizationObservation,
    ) -> Result<(), LaunchError> {
        if *self
            .0
            .convergence
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
        {
            return Err(LaunchError::ContainmentFailure);
        }
        let mut state = self
            .0
            .characterization_observation
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
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
            let mut requested = self
                .0
                .requested_exit
                .lock()
                .map_err(|_| LaunchError::ContainmentFailure)?;
            requested.get_or_insert(reason);
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
        let mut code = 0_u32;
        // SAFETY: the retained process handle is live and code is writable.
        if unsafe { GetExitCodeProcess(self.0.process.raw(), &mut code) } == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        let requested = *self
            .0
            .requested_exit
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
        Ok(requested.unwrap_or(TreeExit::Exited {
            code: Some(code as i32),
        }))
    }

    pub(crate) fn wait(&self) -> Result<TreeExit, LaunchError> {
        self.wait_until(Instant::now() + self.0.terminate_deadline)
    }

    pub(crate) fn wait_before(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        self.wait_until(deadline)
    }

    pub(crate) fn terminate(&self, reason: TreeExit) -> Result<TreeExit, LaunchError> {
        let deadline = Instant::now() + self.0.terminate_deadline;
        self.request_termination(reason)?;
        self.wait_until(deadline)
    }

    pub(crate) fn wait_zero(&self) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let exit = self.wait_until(Instant::now() + self.0.terminate_deadline)?;
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

    pub(crate) fn terminate_zero(
        &self,
        reason: TreeExit,
    ) -> Result<(TreeExit, ZeroProcessToken), LaunchError> {
        let deadline = Instant::now() + self.0.terminate_deadline;
        self.request_termination(reason)?;
        let exit = self.wait_until(deadline)?;
        Ok((exit, ZeroProcessToken::new(self.0.runtime_epoch)))
    }

    pub(crate) fn stderr_summary(&self) -> Result<SafeDiagnostic, LaunchError> {
        self.0.stderr.summary()
    }

    fn wait_until(&self, deadline: Instant) -> Result<TreeExit, LaunchError> {
        let exit = self.wait_root_exit_until(deadline)?;
        self.0.converge_until(deadline)?;
        Ok(exit)
    }

    #[cfg(test)]
    pub(crate) fn active_processes(&self) -> Result<u32, LaunchError> {
        self.0.job.active_processes(self.0.active_process_limit)
    }

    #[cfg(test)]
    pub(crate) fn total_processes(&self) -> Result<u32, LaunchError> {
        self.0.job.total_processes()
    }
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
    #[cfg(any(test, feature = "acp-fixture"))]
    pub(super) absolute_cleanup_deadline: Option<Instant>,
    pub(super) requested_exit: Mutex<Option<TreeExit>>,
    pub(super) convergence: Mutex<bool>,
    #[cfg(feature = "acp-fixture")]
    pub(super) characterization_observation: Mutex<CharacterizationObservationState>,
    pub(super) runtime_epoch: u64,
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
        let mut converged = self
            .convergence
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?;
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
                let mut state = self
                    .characterization_observation
                    .lock()
                    .map_err(|_| LaunchError::ContainmentFailure)?;
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
        let lease = self
            .lease
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
            .take()
            .ok_or(LaunchError::ContainmentFailure)?;
        lease.release_after_zero()?;
        *converged = true;
        Ok(())
    }

    fn shutdown(&self) -> Result<(), LaunchError> {
        let native_deadline = Instant::now() + self.terminate_deadline;
        #[cfg(any(test, feature = "acp-fixture"))]
        let deadline = self
            .absolute_cleanup_deadline
            .map_or(native_deadline, |absolute| native_deadline.min(absolute));
        #[cfg(not(any(test, feature = "acp-fixture")))]
        let deadline = native_deadline;
        if *self
            .convergence
            .lock()
            .map_err(|_| LaunchError::ContainmentFailure)?
        {
            return Ok(());
        }
        {
            let mut requested = self
                .requested_exit
                .lock()
                .map_err(|_| LaunchError::ContainmentFailure)?;
            requested.get_or_insert(TreeExit::ParentShutdown);
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
