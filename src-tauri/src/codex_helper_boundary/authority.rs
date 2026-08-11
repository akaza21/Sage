use crate::codex_helper_boundary::protocol::{BoundaryError, BoundaryStatus};

pub(crate) struct AuthorityObservation {
    pub(crate) expected_caller_pid: u32,
    pub(crate) observed_caller_pid: u32,
    pub(crate) expected_caller_sid_sha256: [u8; 32],
    pub(crate) observed_caller_sid_sha256: [u8; 32],
    pub(crate) caller_elevated: bool,
    pub(crate) helper_elevated: bool,
}

pub(crate) fn boundary_status(
    observation: &AuthorityObservation,
) -> Result<BoundaryStatus, BoundaryError> {
    if observation.expected_caller_pid == 0
        || observation.expected_caller_pid != observation.observed_caller_pid
        || observation.expected_caller_sid_sha256 != observation.observed_caller_sid_sha256
        || observation.caller_elevated
    {
        return Err(BoundaryError::Authority);
    }
    Ok(if observation.helper_elevated {
        BoundaryStatus::ExecutorUnavailable
    } else {
        BoundaryStatus::ElevationRequired
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn exact() -> AuthorityObservation {
        AuthorityObservation {
            expected_caller_pid: 41,
            observed_caller_pid: 41,
            expected_caller_sid_sha256: [0x11; 32],
            observed_caller_sid_sha256: [0x11; 32],
            caller_elevated: false,
            helper_elevated: false,
        }
    }

    #[test]
    fn exact_unelevated_process_authority_requires_elevation() {
        assert_eq!(
            boundary_status(&exact()),
            Ok(BoundaryStatus::ElevationRequired)
        );
    }

    #[test]
    fn pid_sid_and_caller_elevation_drift_are_rejected() {
        let mut pid = exact();
        pid.observed_caller_pid += 1;
        let mut sid = exact();
        sid.observed_caller_sid_sha256[0] ^= 1;
        let mut elevated = exact();
        elevated.caller_elevated = true;
        for changed in [pid, sid, elevated] {
            assert_eq!(boundary_status(&changed), Err(BoundaryError::Authority));
        }
    }

    #[test]
    fn elevated_helper_still_has_no_executor_authority() {
        let mut exact = exact();
        exact.helper_elevated = true;
        assert_eq!(
            boundary_status(&exact),
            Ok(BoundaryStatus::ExecutorUnavailable)
        );
    }
}
