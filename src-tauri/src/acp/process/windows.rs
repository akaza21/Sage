pub(super) mod artifact;
mod authority;
mod batch;
mod clock;
#[cfg(test)]
mod descendant_settlement_tests;
mod encoding;
mod io;
mod native;
mod observe;
mod preview;
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
use self::encoding::{cmd_batch_launcher_command_line, cmd_working_directory};
pub(crate) use self::encoding::{command_line, environment_block, wide_z};
pub(crate) use self::io::{StdinEndpoint, StdoutEndpoint};
use self::native::StdioSet;
use self::observe::{observe_exit, try_observe_exit, wait_until};
#[cfg(test)]
pub(crate) use self::preview::PreviewJobBoundary;
pub(crate) use self::preview::{
    PreviewLaunchFailure, PreviewLaunchRecovery, PreviewWindowsProcess,
};
#[cfg(test)]
pub(crate) use self::preview_recovery_tests::retained_preview_recovery_with_retry_fault_for_test;
pub(crate) use self::recovery::LaunchRecovery;
use self::recovery::{classify_started_failure, fail_pre_spawn};
use super::capacity::TreeCapacityLease;
use super::identity::SpawnTimingReceipt;
use super::platform::LaunchFailure;
use super::supervisor::{EntryPointKind, LaunchSpec, RequiredContainment};
use sage_process::{error::LaunchError, handle::NativeHandle, job::JobBoundary};

const MAX_WINDOWS_WIDE: usize = 32_767;

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
    let (artifact, manifest_guard) = pre_spawn!(VerifiedArtifact::open_with_manifest(spec));
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
    let (program, args) = match spec.artifact.entrypoint_kind {
        EntryPointKind::NativeExecutable => (
            artifact.canonical_path.clone(),
            spec.fixed_args.as_slice().to_vec(),
        ),
        EntryPointKind::WindowsBatchLauncher => {
            let plan = pre_spawn!(parse_npm_codex_launcher(
                &artifact,
                manifest_guard.as_ref(),
                &spec.environment,
                &spec.fixed_args
            ));
            (plan.node, plan.args)
        }
    };
    // The child sees ordinary paths: Node reports its own executable path
    // from what it was started with, and runtimes and harnesses resolve,
    // fork or name folders after these paths. Sage keeps the verbatim
    // canonical paths only for its own identity checks.
    let (application_path, args) = sage_core::win32_path::ordinary_invocation(&program, &args);
    let mut command_line = pre_spawn!(command_line(&application_path, &args));
    let environment = pre_spawn!(environment_block(spec.environment.as_slice()));
    let application = pre_spawn!(wide_z(application_path.as_os_str()));
    // Harnesses derive state from their working directory (Pi names its
    // session folder after it), so they must see the root's ordinary spelling
    // rather than the verbatim canonical one.
    let cwd_path = sage_core::win32_path::simplified(spec.project_root.as_path());
    let cwd = pre_spawn!(wide_z(cwd_path.as_os_str()));
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
