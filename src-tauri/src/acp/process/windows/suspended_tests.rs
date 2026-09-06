use std::cell::RefCell;

use super::LaunchError;
use super::suspended::{converge_exact, finish_before_resume};

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
