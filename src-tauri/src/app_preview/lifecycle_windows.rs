use std::{os::windows::io::AsRawHandle, process::Child, time::Duration};

use windows_sys::Win32::{
    Foundation::CloseHandle,
    System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    },
    System::Threading::WaitForSingleObject,
};

pub(crate) struct PreviewJob(windows_sys::Win32::Foundation::HANDLE);
const CHILD_EXIT_WAIT: Duration = Duration::from_secs(5);

// SAFETY: The HANDLE is an owned Windows job object used only while the registry mutex is held.
// The OS handle may be signaled from any thread, and we never share the raw pointer without
// that lock.
unsafe impl Send for PreviewJob {}

impl PreviewJob {
    pub(crate) fn new(child: &Child) -> Result<Self, ()> {
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(());
        }
        let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
        limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        let configured = unsafe {
            SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::addr_of!(limits).cast(),
                std::mem::size_of_val(&limits) as u32,
            ) != 0
        };
        let assigned =
            configured && unsafe { AssignProcessToJobObject(job, child.as_raw_handle() as _) != 0 };
        if assigned {
            Ok(Self(job))
        } else {
            unsafe {
                TerminateJobObject(job, 1);
                CloseHandle(job);
            }
            Err(())
        }
    }

    pub(crate) fn terminate(&self) {
        unsafe {
            TerminateJobObject(self.0, 1);
            WaitForSingleObject(self.0, CHILD_EXIT_WAIT.as_millis() as u32);
        }
    }
}

impl Drop for PreviewJob {
    fn drop(&mut self) {
        unsafe { CloseHandle(self.0) };
    }
}
