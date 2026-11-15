use std::cell::RefCell;
use std::io::Read;

use windows_sys::Win32::Foundation::WAIT_OBJECT_0;
use windows_sys::Win32::System::Threading::{ResumeThread, WaitForSingleObject};

use super::LaunchError;
use super::suspended::{SuspendedLaunch, converge_exact, finish_before_resume};
use super::{command_line, wide_z};
use crate::console_window_fixture::{FIXTURE_ARGUMENT, assert_launch_has_no_console_window};

const FIXTURE_EXIT_TIMEOUT_MS: u32 = 15_000;

#[test]
fn cleanup_failure_is_retained_as_the_launch_result() {
    assert_eq!(
        finish_before_resume(
            || Err(LaunchError::IdentityMismatch),
            || Ok(()),
            || Ok(()),
            || Err(LaunchError::Deadline),
        ),
        Err(LaunchError::Deadline)
    );
}

#[test]
fn exact_recovery_waits_for_job_zero_before_reporting_job_failure() {
    let steps = RefCell::new(Vec::new());
    assert_eq!(
        converge_exact(
            true,
            || {
                steps.borrow_mut().push("terminate-job");
                Err(LaunchError::ContainmentFailure)
            },
            || {
                steps.borrow_mut().push("reap-root");
                Ok(())
            },
            || {
                steps.borrow_mut().push("job-zero");
                Ok(())
            },
        ),
        Err(LaunchError::ContainmentFailure)
    );
    assert_eq!(
        *steps.into_inner(),
        ["terminate-job", "reap-root", "job-zero"]
    );
}

#[test]
fn agent_launch_starts_a_console_harness_without_a_console_window() {
    let test_path = module_path!().split_once("::").unwrap().1;
    assert_launch_has_no_console_window(
        &format!("{test_path}::agent_launch_starts_a_console_harness_without_a_console_window"),
        |executable| {
            let application = wide_z(executable.as_os_str()).unwrap();
            let mut command_line = command_line(executable, &[FIXTURE_ARGUMENT.into()]).unwrap();
            let cwd = wide_z(executable.parent().unwrap().as_os_str()).unwrap();
            let Ok(launch) = SuspendedLaunch::create(&application, &mut command_line, &cwd, None)
            else {
                panic!("the console fixture could not be created");
            };
            let (process, thread, stdio) = launch.into_parts();
            let (_stdin, mut stdout, _stderr) = stdio.into_parent();
            // SAFETY: `thread` is the live primary thread handle of the
            // suspended fixture this test owns.
            assert_ne!(unsafe { ResumeThread(thread.raw()) }, u32::MAX);
            let mut output = Vec::new();
            stdout.read_to_end(&mut output).unwrap();
            // SAFETY: `process` is the live fixture handle this test owns.
            let exited = unsafe { WaitForSingleObject(process.raw(), FIXTURE_EXIT_TIMEOUT_MS) };
            assert_eq!(exited, WAIT_OBJECT_0);
            output
        },
    );
}
