mod artifact;
mod authority;
mod batch;
mod clock;
#[cfg(test)]
mod descendant_settlement_tests;
mod encoding;
mod io;
mod job;
mod native;
mod observe;
#[cfg(test)]
mod preview_recovery_tests;
mod recovery;
mod suspended;
#[cfg(test)]
mod suspended_tests;

use std::fs::File;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::Threading::ResumeThread;

use self::artifact::VerifiedArtifact;
pub(crate) use self::authority::{RunningTree, TreeControl};
use self::batch::parse_npm_codex_launcher;
pub(crate) use self::clock::monotonic_tick;
use self::clock::performance_clock;
use self::encoding::{
    cmd_batch_launcher_command_line, cmd_working_directory, command_line, environment_block, wide_z,
};
pub(crate) use self::io::{StdinEndpoint, StdoutEndpoint};
use self::job::JobBoundary;
use self::native::{NativeHandle, StdioSet};
use self::observe::{observe_exit, try_observe_exit, wait_until};
#[cfg(test)]
pub(crate) use self::preview_recovery_tests::retained_preview_recovery_with_retry_fault_for_test;
pub(crate) use self::recovery::LaunchRecovery;
use self::recovery::{classify_started_failure, fail_pre_spawn};
use super::capacity::TreeCapacityLease;
use super::identity::SpawnTimingReceipt;
use super::platform::LaunchFailure;
use super::supervisor::{EntryPointKind, LaunchError, LaunchSpec, RequiredContainment};

const MAX_WINDOWS_WIDE: usize = 32_767;

/// Preview reuses the proven Job accounting/termination primitive without
/// inheriting ACP artifact, binding, or batch-launch authority.
pub(crate) struct PreviewJobBoundary(JobBoundary);

impl PreviewJobBoundary {
    pub(crate) fn create() -> Result<Self, LaunchError> {
        Ok(Self(JobBoundary::create(
            u32::try_from(
                crate::acp::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
        )?))
    }

    pub(crate) fn assign_process(&self, process: HANDLE) -> Result<(), LaunchError> {
        self.0.assign(process)
    }

    pub(crate) fn terminate_and_wait_zero(&self, deadline: Instant) -> Result<(), LaunchError> {
        self.0.terminate()?;
        self.0.wait_for_zero(
            u32::try_from(
                crate::acp::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
            deadline,
        )
    }

    fn verify_exactly_one_member(&self) -> Result<(), LaunchError> {
        match self.0.active_processes(
            u32::try_from(
                crate::acp::limits::LIMITS
                    .process
                    .windows_job_active_process_limit,
            )
            .map_err(|_| LaunchError::ContainmentFailure)?,
        )? {
            1 => Ok(()),
            _ => Err(LaunchError::ContainmentFailure),
        }
    }
}

/// Preview-private Windows launch result. This deliberately accepts only the
/// curated `npm run {dev,start}` policy and never exposes ACP's artifact or
/// binding authority.
pub(crate) struct PreviewWindowsProcess {
    process: NativeHandle,
    job: PreviewJobBoundary,
}

pub(crate) enum PreviewLaunchFailure {
    Clean(LaunchError),
    Retained(PreviewLaunchRecovery),
}

impl From<LaunchError> for PreviewLaunchFailure {
    fn from(error: LaunchError) -> Self {
        Self::Clean(error)
    }
}

/// Exact pre-resume authority retained when bounded convergence has not yet
/// proven zero. Callers must retry or retain this value; dropping it only
/// invokes the Job's kill-on-close backstop and never reports success.
pub(crate) struct PreviewLaunchRecovery {
    process: NativeHandle,
    thread: Option<NativeHandle>,
    stdio: Option<StdioSet>,
    job: PreviewJobBoundary,
    assigned: bool,
    #[cfg(test)]
    retry_zero_fault: Option<LaunchError>,
}

impl PreviewLaunchRecovery {
    pub(crate) fn retry_zero(&mut self) -> Result<(), LaunchError> {
        #[cfg(test)]
        if let Some(error) = self.retry_zero_fault.take() {
            return Err(error);
        }
        suspended::terminate_and_reap_exact(
            &self.job.0,
            self.process.raw(),
            self.assigned,
            crate::acp::limits::LIMITS
                .process
                .windows_job_active_process_limit,
            Instant::now()
                + Duration::from_millis(
                    crate::acp::limits::PREVIEW_LIMITS.process_termination_deadline_ms,
                ),
        )?;
        self.stdio.take();
        self.thread.take();
        Ok(())
    }

    fn fail(mut self, error: LaunchError) -> PreviewLaunchFailure {
        match self.retry_zero() {
            Ok(()) => PreviewLaunchFailure::Clean(error),
            Err(_) => PreviewLaunchFailure::Retained(self),
        }
    }
}

// Both owned kernel handles are safe to move, but never copied, across the
// lifecycle registry's mutex boundary.
unsafe impl Send for PreviewWindowsProcess {}

impl PreviewWindowsProcess {
    pub(crate) fn launch_curated_npm(
        package_root: &Path,
        script: &'static str,
    ) -> Result<(Self, File, File), PreviewLaunchFailure> {
        if !matches!(script, "dev" | "start") {
            return Err(LaunchError::InvalidArgument.into());
        }
        let package_root = package_root
            .canonicalize()
            .map_err(|_| LaunchError::InvalidAbsolutePath)?;
        let cmd = preview_cmd_exe()?;
        let launcher = preview_npm_cmd()?;
        let expected_launcher = launcher_metadata(&launcher)?;
        let manifest = package_root.join("package.json");
        let expected_manifest = launcher_metadata(&manifest)?;
        let job = PreviewJobBoundary::create()?;
        let application = wide_z(cmd.as_os_str())?;
        // This buffer is intentionally mutable: CreateProcessW may edit it.
        let mut command_line = cmd_batch_launcher_command_line(
            &cmd,
            &launcher,
            &[
                std::ffi::OsString::from("run"),
                std::ffi::OsString::from(script),
            ],
        )?;
        let cwd = wide_z(&cmd_working_directory(&package_root)?)?;
        let suspended =
            match suspended::SuspendedLaunch::create(&application, &mut command_line, &cwd, None) {
                Ok(suspended) => suspended,
                Err(suspended::SuspendedCreateFailure::Clean(error)) => return Err(error.into()),
                Err(suspended::SuspendedCreateFailure::Retained {
                    error,
                    process,
                    stdio,
                }) => {
                    return Err(PreviewLaunchRecovery {
                        process,
                        thread: None,
                        stdio: Some(stdio),
                        job,
                        assigned: false,
                        #[cfg(test)]
                        retry_zero_fault: None,
                    }
                    .fail(error));
                }
            };
        let (process, thread, stdio) = suspended.into_parts();
        let mut recovery = PreviewLaunchRecovery {
            process,
            thread: Some(thread),
            stdio: Some(stdio),
            job,
            assigned: false,
            #[cfg(test)]
            retry_zero_fault: None,
        };
        let outcome = (|| {
            recovery.job.assign_process(recovery.process.raw())?;
            recovery.assigned = true;
            recovery.job.verify_exactly_one_member()?;
            (launcher_metadata(&launcher)? == expected_launcher
                && launcher_metadata(&manifest)? == expected_manifest)
                .then_some(())
                .ok_or(LaunchError::IdentityMismatch)?;
            let thread = recovery
                .thread
                .as_ref()
                .ok_or(LaunchError::ContainmentFailure)?;
            // SAFETY: this is CreateProcessW's original primary-thread handle.
            (unsafe { ResumeThread(thread.raw()) } != u32::MAX)
                .then_some(())
                .ok_or(LaunchError::ContainmentFailure)
        })();
        if let Err(error) = outcome {
            return Err(recovery.fail(error));
        }
        let PreviewLaunchRecovery {
            process,
            thread,
            stdio,
            job,
            ..
        } = recovery;
        drop(thread);
        let (stdin, stdout, stderr) = stdio.ok_or(LaunchError::ContainmentFailure)?.into_parent();
        drop(stdin);
        Ok((Self { process, job }, stdout, stderr))
    }

    pub(crate) fn exited(&self) -> Result<bool, LaunchError> {
        try_observe_exit(self.process.raw())
    }

    pub(crate) fn terminate_and_wait_zero(&self, deadline: Instant) -> Result<(), LaunchError> {
        self.job.terminate_and_wait_zero(deadline)?;
        wait_until(self.process.raw(), deadline)
    }
}

fn preview_cmd_exe() -> Result<PathBuf, LaunchError> {
    let cmd = std::env::var_os("ComSpec").ok_or(LaunchError::InvalidAbsolutePath)?;
    let cmd = PathBuf::from(cmd)
        .canonicalize()
        .map_err(|_| LaunchError::InvalidAbsolutePath)?;
    cmd.is_file()
        .then_some(cmd)
        .ok_or(LaunchError::InvalidAbsolutePath)
}

fn preview_npm_cmd() -> Result<PathBuf, LaunchError> {
    let path = std::env::var_os("PATH").ok_or(LaunchError::InvalidAbsolutePath)?;
    for directory in std::env::split_paths(&path) {
        if !directory.is_absolute() {
            continue;
        }
        let candidate = directory.join("npm.cmd");
        if candidate.is_file() {
            return candidate
                .canonicalize()
                .map_err(|_| LaunchError::InvalidAbsolutePath);
        }
    }
    Err(LaunchError::InvalidAbsolutePath)
}

fn launcher_metadata(path: &Path) -> Result<(u64, std::time::SystemTime), LaunchError> {
    let metadata = path.metadata().map_err(|_| LaunchError::Io)?;
    Ok((
        metadata.len(),
        metadata.modified().map_err(|_| LaunchError::Io)?,
    ))
}

pub(crate) fn launch(
    spec: &LaunchSpec,
    mut lease: TreeCapacityLease,
) -> Result<RunningTree, LaunchFailure> {
    if spec.required_guarantee != RequiredContainment::EnforcedTree
        || spec.limits.active_processes != spec.limits.descendants.saturating_add(1)
        || !lease.matches(spec.limits.descendants)
    {
        return fail_pre_spawn(LaunchError::ContainmentFailure, lease, spec.runtime_epoch);
    }
    let _intent = (
        spec.binding_id,
        spec.runtime_epoch,
        spec.artifact.artifact_id,
        spec.project_root.scope(),
    );
    macro_rules! pre_spawn {
        ($operation:expr) => {
            match $operation {
                Ok(value) => value,
                Err(error) => return fail_pre_spawn(error, lease, spec.runtime_epoch),
            }
        };
    }
    let artifact = pre_spawn!(VerifiedArtifact::open(spec));
    #[cfg(test)]
    if let Some(hook) = &spec.test_after_artifact_verified {
        hook();
    }
    pre_spawn!(
        spec.project_root
            .revalidate()
            .map_err(|_| LaunchError::IdentityMismatch)
    );
    let job = pre_spawn!(JobBoundary::create(spec.limits.active_processes));
    let (application_path, mut command_line, cwd_path) = match spec.artifact.entrypoint_kind {
        EntryPointKind::NativeExecutable => (
            artifact.canonical_path.clone(),
            pre_spawn!(command_line(
                &artifact.canonical_path,
                spec.fixed_args.as_slice()
            )),
            spec.project_root.as_path().as_os_str().to_owned(),
        ),
        EntryPointKind::WindowsBatchLauncher => {
            let plan = pre_spawn!(parse_npm_codex_launcher(
                &artifact,
                &spec.environment,
                &spec.fixed_args
            ));
            (
                plan.node.clone(),
                pre_spawn!(command_line(&plan.node, &plan.args)),
                spec.project_root.as_path().as_os_str().to_owned(),
            )
        }
    };
    let environment = pre_spawn!(environment_block(spec.environment.as_slice()));
    let application = pre_spawn!(wide_z(application_path.as_os_str()));
    let cwd = pre_spawn!(wide_z(&cwd_path));
    // Batch launchers remain writable because npm may update them concurrently.
    // Ensure the exact discovered launcher has not drifted after parsing it and
    // before node receives the derived Codex script path.
    pre_spawn!(artifact.revalidate(spec, "pre-spawn .cmd"));
    let (frequency_hz, spawn_started_tick) = pre_spawn!(performance_clock());
    let suspended = match suspended::SuspendedLaunch::create(
        &application,
        &mut command_line,
        &cwd,
        Some(&environment),
    ) {
        Ok(suspended) => suspended,
        Err(suspended::SuspendedCreateFailure::Clean(error)) => {
            return fail_pre_spawn(error, lease, spec.runtime_epoch);
        }
        Err(suspended::SuspendedCreateFailure::Retained {
            error,
            process,
            stdio,
        }) => {
            drop(stdio);
            lease.activate();
            let recovery = LaunchRecovery::child_created(
                Some(process),
                None,
                job,
                artifact,
                lease,
                None,
                spec,
            );
            return Err(classify_started_failure(error, recovery));
        }
    };
    let process_ready_tick = monotonic_tick();
    lease.activate();
    let (process, thread, stdio) = suspended.into_parts();
    let spawn_timing = match process_ready_tick
        .and_then(|ready| SpawnTimingReceipt::new(frequency_hz, spawn_started_tick, ready))
    {
        Ok(receipt) => receipt,
        Err(error) => {
            let recovery = LaunchRecovery::child_created(
                Some(process),
                Some(thread),
                job,
                artifact,
                lease,
                None,
                spec,
            );
            return Err(classify_started_failure(error, recovery));
        }
    };
    let (parent_stdin, parent_stdout, parent_stderr) = stdio.into_parent();

    let mut recovery = LaunchRecovery::child_created(
        Some(process),
        Some(thread),
        job,
        artifact,
        lease,
        Some(spawn_timing),
        spec,
    );
    if let Err(error) = recovery.assign_job() {
        return Err(classify_started_failure(error, recovery));
    }
    if let Err(error) = recovery.verify_artifact(spec, &application_path) {
        return Err(classify_started_failure(error, recovery));
    }
    if let Err(error) = recovery.start_stderr(parent_stderr) {
        return Err(classify_started_failure(error, recovery));
    }
    if let Err(error) = recovery.resume() {
        return Err(classify_started_failure(error, recovery));
    }
    recovery.use_termination_deadline(spec);
    recovery.into_running(parent_stdin, parent_stdout)
}
