mod artifact;
mod authority;
mod encoding;
mod io;
mod job;
mod native;
mod recovery;

use std::mem::size_of;
use std::ptr::null;
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, EXTENDED_STARTUPINFO_PRESENT,
    INFINITE, PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, WaitForSingleObject,
};

use self::artifact::VerifiedArtifact;
pub(crate) use self::authority::{RunningTree, TreeControl};
use self::encoding::{command_line, environment_block, wide_z};
pub(crate) use self::io::{StdinEndpoint, StdoutEndpoint};
use self::job::JobBoundary;
use self::native::{AttributeList, NativeHandle, StdioSet};
pub(crate) use self::recovery::LaunchRecovery;
use self::recovery::{classify_started_failure, fail_pre_spawn};
use super::capacity::TreeCapacityLease;
use super::identity::SpawnTimingReceipt;
use super::platform::LaunchFailure;
use super::supervisor::{LaunchError, LaunchSpec, RequiredContainment};

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
    let stdio = pre_spawn!(StdioSet::create());
    let child_handles = stdio.child_handles();
    let attributes = pre_spawn!(AttributeList::handles(&child_handles));
    let mut command_line = pre_spawn!(command_line(
        &artifact.canonical_path,
        spec.fixed_args.as_slice()
    ));
    let environment = pre_spawn!(environment_block(spec.environment.as_slice()));
    let application = pre_spawn!(wide_z(artifact.canonical_path.as_os_str()));
    let cwd = pre_spawn!(wide_z(spec.project_root.as_path().as_os_str()));
    let mut startup = STARTUPINFOEXW::default();
    startup.StartupInfo.cb = pre_spawn!(
        u32::try_from(size_of::<STARTUPINFOEXW>()).map_err(|_| LaunchError::ContainmentFailure)
    );
    startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
    startup.StartupInfo.hStdInput = child_handles[0];
    startup.StartupInfo.hStdOutput = child_handles[1];
    startup.StartupInfo.hStdError = child_handles[2];
    startup.lpAttributeList = attributes.pointer;
    let mut info = PROCESS_INFORMATION::default();
    let (frequency_hz, spawn_started_tick) = pre_spawn!(performance_clock());
    // SAFETY: pointers are valid and the attribute list restricts inheritance.
    if unsafe {
        CreateProcessW(
            application.as_ptr(),
            command_line.as_mut_ptr(),
            null(),
            null(),
            1,
            CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
            environment.as_ptr().cast(),
            cwd.as_ptr(),
            (&raw const startup.StartupInfo),
            &mut info,
        )
    } == 0
    {
        return fail_pre_spawn(LaunchError::Io, lease, spec.runtime_epoch);
    }
    let process_ready_tick = monotonic_tick();
    lease.activate();
    let spawn_timing = match process_ready_tick
        .and_then(|ready| SpawnTimingReceipt::new(frequency_hz, spawn_started_tick, ready))
    {
        Ok(receipt) => receipt,
        Err(error) => {
            close_thread_handle(info.hThread);
            let process = NativeHandle::new(info.hProcess).ok();
            let recovery =
                LaunchRecovery::child_created(process, None, job, artifact, lease, None, spec);
            return Err(classify_started_failure(error, recovery));
        }
    };
    let process = match NativeHandle::new(info.hProcess) {
        Ok(process) => process,
        Err(error) => {
            close_thread_handle(info.hThread);
            return Err(LaunchFailure::convergence_unknown(
                error,
                LaunchRecovery::child_created(
                    None,
                    None,
                    job,
                    artifact,
                    lease,
                    Some(spawn_timing),
                    spec,
                ),
            ));
        }
    };
    let thread = match NativeHandle::new(info.hThread) {
        Ok(thread) => thread,
        Err(error) => {
            let recovery = LaunchRecovery::child_created(
                Some(process),
                None,
                job,
                artifact,
                lease,
                Some(spawn_timing),
                spec,
            );
            return Err(classify_started_failure(error, recovery));
        }
    };
    drop(attributes);
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
    if let Err(error) = recovery.verify_artifact(spec) {
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

fn performance_clock() -> Result<(u64, u64), LaunchError> {
    let mut frequency = 0_i64;
    // SAFETY: the output points to initialized writable storage.
    if unsafe { QueryPerformanceFrequency(&mut frequency) } == 0 || frequency <= 0 {
        return Err(LaunchError::ContainmentFailure);
    }
    Ok((
        u64::try_from(frequency).map_err(|_| LaunchError::ContainmentFailure)?,
        monotonic_tick()?,
    ))
}

pub(crate) fn monotonic_tick() -> Result<u64, LaunchError> {
    let mut tick = 0_i64;
    // SAFETY: the output points to initialized writable storage.
    if unsafe { QueryPerformanceCounter(&mut tick) } == 0 || tick < 0 {
        return Err(LaunchError::ContainmentFailure);
    }
    u64::try_from(tick).map_err(|_| LaunchError::ContainmentFailure)
}

fn close_thread_handle(handle: HANDLE) {
    if !handle.is_null() {
        // SAFETY: CreateProcessW returned this otherwise unowned handle.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(handle) };
    }
}

fn wait_handle(handle: HANDLE, timeout: Duration) -> Result<(), LaunchError> {
    let milliseconds = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    // SAFETY: handle is live and timeout is bounded.
    match unsafe { WaitForSingleObject(handle, milliseconds) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(LaunchError::Deadline),
        _ => Err(LaunchError::ContainmentFailure),
    }
}

fn wait_until(handle: HANDLE, deadline: Instant) -> Result<(), LaunchError> {
    wait_handle(handle, deadline.saturating_duration_since(Instant::now()))
}

fn observe_exit(handle: HANDLE) -> Result<(), LaunchError> {
    // The watcher owns no teardown authority. The retained TreeControl does,
    // and every runner exit requests job termination before joining this wait.
    // SAFETY: the process handle remains live through the shared authority.
    match unsafe { WaitForSingleObject(handle, INFINITE) } {
        WAIT_OBJECT_0 => Ok(()),
        _ => Err(LaunchError::ContainmentFailure),
    }
}
