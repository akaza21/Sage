use std::mem::size_of;
use std::ptr::{null, null_mut};
use std::time::{Duration, Instant};

use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_ACTIVE_PROCESS,
    JOB_OBJECT_LIMIT_BREAKAWAY_OK, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION,
    JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectBasicAccountingInformation,
    JobObjectExtendedLimitInformation, QueryInformationJobObject, SetInformationJobObject,
    TerminateJobObject,
};

use super::super::supervisor::LaunchError;
use super::native::NativeHandle;

pub(super) struct JobBoundary(NativeHandle);

impl JobBoundary {
    pub(super) fn create(active_limit: u32) -> Result<Self, LaunchError> {
        // SAFETY: null security/name creates an unnamed non-inheritable Job.
        let job = NativeHandle::new(unsafe { CreateJobObjectW(null(), null()) })
            .map_err(|_| LaunchError::ContainmentFailure)?;
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags =
            JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE | JOB_OBJECT_LIMIT_ACTIVE_PROCESS;
        limits.BasicLimitInformation.ActiveProcessLimit = active_limit;
        // SAFETY: the information class and structure layout match exactly.
        if unsafe {
            SetInformationJobObject(
                job.raw(),
                JobObjectExtendedLimitInformation,
                (&raw const limits).cast(),
                u32::try_from(size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .map_err(|_| LaunchError::ContainmentFailure)?,
            )
        } == 0
        {
            return Err(LaunchError::ContainmentFailure);
        }
        verify_job(job.raw(), active_limit)?;
        Ok(Self(job))
    }

    pub(super) fn assign(&self, process: HANDLE) -> Result<(), LaunchError> {
        // SAFETY: both handles are live and assignment happens while suspended.
        if unsafe { AssignProcessToJobObject(self.0.raw(), process) } == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(())
    }

    pub(super) fn terminate(&self) -> Result<(), LaunchError> {
        // SAFETY: the Job is live and exclusively controlled by this supervisor.
        if unsafe { TerminateJobObject(self.0.raw(), 1) } == 0 {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(())
    }

    /// Job accounting is the membership authority. No PID is reopened, so a
    /// recycled system PID can never make teardown wait on an unrelated process.
    pub(super) fn active_processes(&self, capacity: u32) -> Result<u32, LaunchError> {
        let accounting = self.accounting()?;
        if accounting.ActiveProcesses > capacity {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(accounting.ActiveProcesses)
    }

    #[cfg(test)]
    pub(super) fn total_processes(&self) -> Result<u32, LaunchError> {
        Ok(self.accounting()?.TotalProcesses)
    }

    fn accounting(&self) -> Result<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, LaunchError> {
        let mut accounting = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
        // SAFETY: accounting is writable and the information class matches it.
        if unsafe {
            QueryInformationJobObject(
                self.0.raw(),
                JobObjectBasicAccountingInformation,
                (&raw mut accounting).cast(),
                u32::try_from(size_of::<JOBOBJECT_BASIC_ACCOUNTING_INFORMATION>())
                    .map_err(|_| LaunchError::ContainmentFailure)?,
                null_mut(),
            )
        } == 0
        {
            return Err(LaunchError::ContainmentFailure);
        }
        Ok(accounting)
    }

    /// Teardown-only bounded convergence. No steady-state timer or polling is
    /// created; Job accounting remains the sole membership authority.
    pub(super) fn wait_for_zero(
        &self,
        capacity: u32,
        deadline: Instant,
    ) -> Result<(), LaunchError> {
        bounded_zero_wait(
            || self.active_processes(capacity),
            deadline,
            || std::thread::sleep(Duration::from_millis(1)),
        )
    }
}

fn bounded_zero_wait(
    mut active: impl FnMut() -> Result<u32, LaunchError>,
    deadline: Instant,
    mut pause: impl FnMut(),
) -> Result<(), LaunchError> {
    loop {
        if active()? == 0 {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(LaunchError::Deadline);
        }
        pause();
    }
}

fn verify_job(job: HANDLE, active_limit: u32) -> Result<(), LaunchError> {
    let mut actual = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    // SAFETY: actual is writable and the class matches its layout.
    if unsafe {
        QueryInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            (&raw mut actual).cast(),
            u32::try_from(size_of_val(&actual)).map_err(|_| LaunchError::ContainmentFailure)?,
            null_mut(),
        )
    } == 0
    {
        return Err(LaunchError::ContainmentFailure);
    }
    let flags = actual.BasicLimitInformation.LimitFlags;
    let forbidden = JOB_OBJECT_LIMIT_BREAKAWAY_OK | JOB_OBJECT_LIMIT_SILENT_BREAKAWAY_OK;
    if flags & JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE == 0
        || flags & JOB_OBJECT_LIMIT_ACTIVE_PROCESS == 0
        || flags & forbidden != 0
        || actual.BasicLimitInformation.ActiveProcessLimit != active_limit
    {
        return Err(LaunchError::ContainmentFailure);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn teardown_wait_reaches_exact_zero_and_times_out_nonzero() {
        let mut observations = [2_u32, 1, 0].into_iter();
        assert_eq!(
            bounded_zero_wait(
                || Ok(observations.next().unwrap_or(0)),
                Instant::now() + Duration::from_secs(1),
                || {}
            ),
            Ok(())
        );
        assert_eq!(
            bounded_zero_wait(|| Ok(1), Instant::now(), || {}),
            Err(LaunchError::Deadline)
        );
    }
}
