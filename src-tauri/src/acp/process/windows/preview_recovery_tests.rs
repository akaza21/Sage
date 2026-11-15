use super::{PreviewJobBoundary, PreviewLaunchRecovery, command_line, suspended, wide_z};
use sage_process::error::LaunchError;

/// Builds retained pre-resume Preview authority for the registry regression.
/// The one-shot fault is local to this recovery; its retry still terminates the
/// exact suspended root and waits for its assigned Job to reach zero.
pub(crate) fn retained_preview_recovery_with_retry_fault_for_test()
-> Result<PreviewLaunchRecovery, LaunchError> {
    let job = PreviewJobBoundary::create()?;
    let executable = std::env::current_exe().map_err(|_| LaunchError::Io)?;
    let cwd = std::env::current_dir().map_err(|_| LaunchError::Io)?;
    let application = wide_z(executable.as_os_str())?;
    let mut command_line = command_line(&executable, &[])?;
    let cwd = wide_z(cwd.as_os_str())?;
    let suspended =
        match suspended::SuspendedLaunch::create(&application, &mut command_line, &cwd, None) {
            Ok(suspended) => suspended,
            Err(suspended::SuspendedCreateFailure::Clean(error)) => return Err(error),
            Err(suspended::SuspendedCreateFailure::Retained {
                error,
                process,
                stdio,
            }) => {
                let recovery = PreviewLaunchRecovery {
                    process,
                    thread: None,
                    stdio: Some(stdio),
                    job,
                    assigned: false,
                    retry_zero_fault: None,
                };
                return Err(clean_test_recovery(recovery, error));
            }
        };
    let (process, thread, stdio) = suspended.into_parts();
    let mut recovery = PreviewLaunchRecovery {
        process,
        thread: Some(thread),
        stdio: Some(stdio),
        job,
        assigned: false,
        retry_zero_fault: None,
    };
    if let Err(error) = recovery.job.assign_process(recovery.process.raw()) {
        return Err(clean_test_recovery(recovery, error));
    }
    recovery.assigned = true;
    if let Err(error) = recovery.job.verify_exactly_one_member() {
        return Err(clean_test_recovery(recovery, error));
    }
    recovery.retry_zero_fault = Some(LaunchError::Deadline);
    Ok(recovery)
}

fn clean_test_recovery(mut recovery: PreviewLaunchRecovery, error: LaunchError) -> LaunchError {
    if recovery.retry_zero().is_ok() {
        return error;
    }
    let _ = recovery.retry_zero();
    drop(recovery);
    LaunchError::ContainmentFailure
}
