use super::authority::{
    AuthorityObservation, ElevatedBoundaryAuthority, PublicationAuthority, publication_authority,
};
use super::protocol::{
    AuthenticatedRequest, BoundaryError, BoundaryStatus, HelperIdentity, OneShotSecret,
    RESPONSE_FRAME_BYTES, seal_response,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PublicationFailure {
    Rejected,
    RecoveryRequired,
}

pub(crate) trait PublicationExecutor {
    fn publish(
        &mut self,
        authority: &ElevatedBoundaryAuthority,
        payload: &[u8],
        secret: &OneShotSecret,
        helper: HelperIdentity,
    ) -> Result<(), PublicationFailure>;

    #[cfg(windows)]
    fn recover(
        &mut self,
        authority: &ElevatedBoundaryAuthority,
        payload: &[u8],
        secret: &OneShotSecret,
        helper: HelperIdentity,
    ) -> Result<(), PublicationFailure> {
        self.publish(authority, payload, secret, helper)
    }

    #[cfg(windows)]
    fn update(
        &mut self,
        authority: &ElevatedBoundaryAuthority,
        payload: &[u8],
        secret: &OneShotSecret,
        helper: HelperIdentity,
    ) -> Result<(), PublicationFailure> {
        self.publish(authority, payload, secret, helper)
    }

    #[cfg(windows)]
    fn remove(
        &mut self,
        authority: &ElevatedBoundaryAuthority,
        payload: &[u8],
        secret: &OneShotSecret,
        helper: HelperIdentity,
    ) -> Result<(), PublicationFailure> {
        self.publish(authority, payload, secret, helper)
    }
}

pub(crate) fn authenticated_response(
    observation: &AuthorityObservation,
    request: &AuthenticatedRequest<'_>,
    secret: &OneShotSecret,
    observed_helper: HelperIdentity,
    executor: &mut impl PublicationExecutor,
) -> Result<[u8; RESPONSE_FRAME_BYTES], BoundaryError> {
    if observed_helper != request.helper {
        return Err(BoundaryError::HelperIdentity);
    }
    let status = match publication_authority(observation)? {
        PublicationAuthority::ElevationRequired => BoundaryStatus::ElevationRequired,
        PublicationAuthority::Elevated(authority) => {
            match executor.publish(&authority, request.payload, secret, observed_helper) {
                Ok(()) => BoundaryStatus::Published,
                Err(PublicationFailure::Rejected) => BoundaryStatus::PublicationFailed,
                Err(PublicationFailure::RecoveryRequired) => BoundaryStatus::RecoveryRequired,
            }
        }
    };
    Ok(seal_response(status, request, secret))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codex_helper_boundary::authority::{
        AuthorityObservation, ElevatedBoundaryAuthority,
    };
    use crate::codex_helper_boundary::protocol::{
        HelperIdentity, OneShotSecret, SealedRequest, decode_request, verify_response,
    };

    struct CountingExecutor {
        calls: usize,
        reject: bool,
    }

    impl PublicationExecutor for CountingExecutor {
        fn publish(
            &mut self,
            _authority: &ElevatedBoundaryAuthority,
            _payload: &[u8],
            _secret: &OneShotSecret,
            _helper: HelperIdentity,
        ) -> Result<(), PublicationFailure> {
            self.calls += 1;
            if self.reject {
                Err(PublicationFailure::Rejected)
            } else {
                Ok(())
            }
        }
    }

    struct RecoveryExecutor;

    impl PublicationExecutor for RecoveryExecutor {
        fn publish(
            &mut self,
            _authority: &ElevatedBoundaryAuthority,
            _payload: &[u8],
            _secret: &OneShotSecret,
            _helper: HelperIdentity,
        ) -> Result<(), PublicationFailure> {
            Err(PublicationFailure::RecoveryRequired)
        }
    }

    #[test]
    fn non_elevated_authenticated_request_returns_elevation_required_without_reaching_executor() {
        let secret = OneShotSecret::from_bytes([0x33; 32]);
        let helper = HelperIdentity {
            sha256: [0x22; 32],
            byte_len: 1_048_576,
        };
        let sealed = SealedRequest::seal(
            41,
            [0x11; 32],
            [0x44; 16],
            helper,
            b"authenticated-core-request",
            &secret,
        )
        .unwrap();
        let request = decode_request(sealed.bytes(), &secret).unwrap();
        let observation = AuthorityObservation {
            expected_caller_pid: 41,
            observed_caller_pid: 41,
            expected_caller_sid_sha256: [0x11; 32],
            observed_caller_sid_sha256: [0x11; 32],
            caller_elevated: false,
            helper_elevated: false,
        };
        let mut executor = CountingExecutor {
            calls: 0,
            reject: false,
        };

        let response =
            authenticated_response(&observation, &request, &secret, helper, &mut executor).unwrap();

        assert_eq!(
            verify_response(&response, &sealed, &secret),
            Ok(crate::codex_helper_boundary::BoundaryStatus::ElevationRequired)
        );
        assert_eq!(executor.calls, 0);
    }

    #[test]
    fn elevated_exact_authority_returns_authenticated_published() {
        let secret = OneShotSecret::from_bytes([0x33; 32]);
        let helper = HelperIdentity {
            sha256: [0x22; 32],
            byte_len: 1_048_576,
        };
        let sealed = SealedRequest::seal(
            41,
            [0x11; 32],
            [0x44; 16],
            helper,
            b"authenticated-core-request",
            &secret,
        )
        .unwrap();
        let request = decode_request(sealed.bytes(), &secret).unwrap();
        let observation = AuthorityObservation {
            expected_caller_pid: 41,
            observed_caller_pid: 41,
            expected_caller_sid_sha256: [0x11; 32],
            observed_caller_sid_sha256: [0x11; 32],
            caller_elevated: false,
            helper_elevated: true,
        };
        let mut executor = CountingExecutor {
            calls: 0,
            reject: false,
        };

        let response =
            authenticated_response(&observation, &request, &secret, helper, &mut executor).unwrap();

        assert_eq!(
            verify_response(&response, &sealed, &secret),
            Ok(crate::codex_helper_boundary::BoundaryStatus::Published)
        );
        assert_eq!(executor.calls, 1);
    }

    #[test]
    fn elevated_executor_rejection_returns_authenticated_generic_failure() {
        let secret = OneShotSecret::from_bytes([0x33; 32]);
        let helper = HelperIdentity {
            sha256: [0x22; 32],
            byte_len: 1_048_576,
        };
        let sealed = SealedRequest::seal(
            41,
            [0x11; 32],
            [0x44; 16],
            helper,
            b"request-containing-secret-and-path-that-must-not-escape",
            &secret,
        )
        .unwrap();
        let request = decode_request(sealed.bytes(), &secret).unwrap();
        let observation = AuthorityObservation {
            expected_caller_pid: 41,
            observed_caller_pid: 41,
            expected_caller_sid_sha256: [0x11; 32],
            observed_caller_sid_sha256: [0x11; 32],
            caller_elevated: false,
            helper_elevated: true,
        };
        let mut executor = CountingExecutor {
            calls: 0,
            reject: true,
        };

        let response =
            authenticated_response(&observation, &request, &secret, helper, &mut executor).unwrap();

        assert_eq!(
            verify_response(&response, &sealed, &secret),
            Ok(crate::codex_helper_boundary::BoundaryStatus::PublicationFailed)
        );
        assert_eq!(
            response.len(),
            crate::codex_helper_boundary::protocol::RESPONSE_FRAME_BYTES
        );
        assert_eq!(executor.calls, 1);
    }

    #[test]
    fn committed_cleanup_outcome_returns_authenticated_recovery_required() {
        let secret = OneShotSecret::from_bytes([0x33; 32]);
        let helper = HelperIdentity {
            sha256: [0x22; 32],
            byte_len: 1_048_576,
        };
        let sealed = SealedRequest::seal(
            41,
            [0x11; 32],
            [0x44; 16],
            helper,
            b"authenticated-core-request",
            &secret,
        )
        .unwrap();
        let request = decode_request(sealed.bytes(), &secret).unwrap();
        let observation = AuthorityObservation {
            expected_caller_pid: 41,
            observed_caller_pid: 41,
            expected_caller_sid_sha256: [0x11; 32],
            observed_caller_sid_sha256: [0x11; 32],
            caller_elevated: false,
            helper_elevated: true,
        };

        let response = authenticated_response(
            &observation,
            &request,
            &secret,
            helper,
            &mut RecoveryExecutor,
        )
        .unwrap();

        assert_eq!(
            verify_response(&response, &sealed, &secret),
            Ok(crate::codex_helper_boundary::BoundaryStatus::RecoveryRequired)
        );
    }
}
