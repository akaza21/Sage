use super::super::supervisor::LaunchError;
use super::job::JobBoundary;
use super::native::{AttributeList, NativeHandle, StdioSet};
use std::mem::size_of;
use std::ptr::null;
use std::time::Instant;
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
use windows_sys::Win32::System::Threading::{
    CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT, CreateProcessW, EXTENDED_STARTUPINFO_PRESENT,
    PROCESS_INFORMATION, STARTF_USESTDHANDLES, STARTUPINFOEXW, TerminateProcess,
    WaitForSingleObject,
};
/// Shared low-level outcome of CreateProcessW. Policy adapters supply only
/// already-validated UTF-16 application, command, cwd, and environment data.
/// The primary thread remains suspended until the caller assigns containment,
/// validates its policy artifact, and explicitly resumes it.
pub(super) struct SuspendedLaunch {
    process: NativeHandle,
    thread: NativeHandle,
    stdio: StdioSet,
}

pub(super) enum SuspendedCreateFailure {
    Clean(LaunchError),
    Retained {
        error: LaunchError,
        process: NativeHandle,
        stdio: StdioSet,
    },
}

impl From<LaunchError> for SuspendedCreateFailure {
    fn from(error: LaunchError) -> Self {
        Self::Clean(error)
    }
}
impl SuspendedLaunch {
    pub(super) fn create(
        application: &[u16],
        command_line: &mut [u16],
        cwd: &[u16],
        environment: Option<&[u16]>,
    ) -> Result<Self, SuspendedCreateFailure> {
        let stdio = StdioSet::create()?;
        let child_handles = stdio.child_handles();
        let attributes = AttributeList::handles(&child_handles)?;
        let mut startup = STARTUPINFOEXW::default();
        startup.StartupInfo.cb = u32::try_from(size_of::<STARTUPINFOEXW>())
            .map_err(|_| LaunchError::ContainmentFailure)?;
        startup.StartupInfo.dwFlags = STARTF_USESTDHANDLES;
        startup.StartupInfo.hStdInput = child_handles[0];
        startup.StartupInfo.hStdOutput = child_handles[1];
        startup.StartupInfo.hStdError = child_handles[2];
        startup.lpAttributeList = attributes.pointer;
        let mut info = PROCESS_INFORMATION::default();
        // SAFETY: caller supplied terminated UTF-16 buffers; this helper owns
        // the restricted stdio and startup attribute list for the call.
        if unsafe {
            CreateProcessW(
                application.as_ptr(),
                command_line.as_mut_ptr(),
                null(),
                null(),
                1,
                CREATE_SUSPENDED | CREATE_UNICODE_ENVIRONMENT | EXTENDED_STARTUPINFO_PRESENT,
                environment.map_or(null(), |value| value.as_ptr().cast()),
                cwd.as_ptr(),
                (&raw const startup.StartupInfo),
                &mut info,
            )
        } == 0
        {
            return Err(LaunchError::Io.into());
        }
        let process = match NativeHandle::new(info.hProcess) {
            Ok(process) => process,
            Err(error) => {
                close_unowned_thread(info.hThread);
                return Err(error.into());
            }
        };
        let thread = match NativeHandle::new(info.hThread) {
            Ok(thread) => thread,
            Err(error) => {
                return Err(SuspendedCreateFailure::Retained {
                    error,
                    process,
                    stdio,
                });
            }
        };
        drop(attributes);
        Ok(Self {
            process,
            thread,
            stdio,
        })
    }
    pub(super) fn into_parts(self) -> (NativeHandle, NativeHandle, StdioSet) {
        (self.process, self.thread, self.stdio)
    }
}
/// Shared fail-closed convergence for a just-created root. The root handle is
/// the authority before Job assignment; after assignment Job accounting must
/// reach zero as well. No PID is queried or rediscovered.
pub(super) fn terminate_and_reap_exact(
    job: &JobBoundary,
    process: HANDLE,
    assigned: bool,
    active_limit: u64,
    deadline: Instant,
) -> Result<(), LaunchError> {
    let active_limit = u32::try_from(active_limit).map_err(|_| LaunchError::ContainmentFailure)?;
    converge_exact(
        assigned,
        || job.terminate(),
        || terminate_exact_root(process, deadline),
        || job.wait_for_zero(active_limit, deadline),
    )
}
pub(super) fn converge_exact(
    assigned: bool,
    terminate_job: impl FnMut() -> Result<(), LaunchError>,
    mut terminate_root: impl FnMut() -> Result<(), LaunchError>,
    mut wait_for_zero: impl FnMut() -> Result<(), LaunchError>,
) -> Result<(), LaunchError> {
    let job_failure = assigned.then(terminate_job).transpose().err();
    terminate_root()?;
    if assigned {
        wait_for_zero()?;
    }
    job_failure.map_or(Ok(()), Err)
}
fn terminate_exact_root(process: HANDLE, deadline: Instant) -> Result<(), LaunchError> {
    // SAFETY: this is the retained CreateProcessW handle, not a reopened PID.
    let _ = unsafe { TerminateProcess(process, 1) };
    wait_for_process(process, deadline)
}

fn wait_for_process(process: HANDLE, deadline: Instant) -> Result<(), LaunchError> {
    let milliseconds = u32::try_from(
        deadline
            .saturating_duration_since(Instant::now())
            .as_millis(),
    )
    .unwrap_or(u32::MAX - 1);
    // SAFETY: process is a live owned handle while this helper is called.
    match unsafe { WaitForSingleObject(process, milliseconds) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(LaunchError::Deadline),
        _ => Err(LaunchError::ContainmentFailure),
    }
}

fn close_unowned_thread(thread: HANDLE) {
    if !thread.is_null() {
        // SAFETY: CreateProcessW returned an otherwise unowned thread handle.
        unsafe { windows_sys::Win32::Foundation::CloseHandle(thread) };
    }
}

/// No policy adapter may resume before its containment and artifact checks.
/// Any failure retains local ownership long enough to run its exact recovery.
#[cfg(test)]
pub(super) fn finish_before_resume(
    mut assign: impl FnMut() -> Result<(), LaunchError>,
    mut validate: impl FnMut() -> Result<(), LaunchError>,
    mut resume: impl FnMut() -> Result<(), LaunchError>,
    mut recover: impl FnMut() -> Result<(), LaunchError>,
) -> Result<(), LaunchError> {
    for step in [
        &mut assign as &mut dyn FnMut() -> Result<(), LaunchError>,
        &mut validate,
        &mut resume,
    ] {
        if let Err(error) = step() {
            recover()?;
            return Err(error);
        }
    }
    Ok(())
}
