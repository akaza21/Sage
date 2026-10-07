use super::*;

#[test]
fn exit_is_deferred_until_bounded_shutdown_succeeds() {
    let prevented = std::cell::Cell::new(0);
    assert_eq!(
        handle_exit_request(
            || Err(crate::acp::runtime::HostError::RecoveryRequired),
            || prevented.set(prevented.get() + 1),
            false,
        ),
        ExitRequestOutcome::Deferred(crate::acp::runtime::HostError::RecoveryRequired)
    );
    assert_eq!(prevented.get(), 1);
    assert_eq!(
        handle_exit_request(|| Ok(()), || prevented.set(99), false),
        ExitRequestOutcome::Allowed
    );
    assert_eq!(prevented.get(), 1);
}

#[test]
#[cfg(windows)]
fn windowless_exit_retries_failed_shutdown_then_allows_process_exit() {
    let attempts = std::cell::Cell::new(0);
    let prevented = std::cell::Cell::new(false);
    let outcome = handle_exit_request(
        || {
            attempts.set(attempts.get() + 1);
            Err(crate::acp::runtime::HostError::Deadline)
        },
        || prevented.set(true),
        true,
    );
    assert_eq!(attempts.get(), 2);
    assert!(!prevented.get());
    assert_eq!(
        outcome,
        ExitRequestOutcome::AllowedAfterFailedRetry {
            first_error: crate::acp::runtime::HostError::Deadline,
            retry_error: crate::acp::runtime::HostError::Deadline,
        }
    );
}

#[test]
#[cfg(windows)]
fn windowless_exit_allows_successful_bounded_retry() {
    let attempts = std::cell::Cell::new(0);
    let prevented = std::cell::Cell::new(false);
    let outcome = handle_exit_request(
        || {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err(crate::acp::runtime::HostError::Deadline)
            } else {
                Ok(())
            }
        },
        || prevented.set(true),
        true,
    );
    assert_eq!(attempts.get(), 2);
    assert!(!prevented.get());
    assert_eq!(
        outcome,
        ExitRequestOutcome::AllowedAfterRetry(crate::acp::runtime::HostError::Deadline)
    );
}

#[test]
#[cfg(not(windows))]
fn posix_windowless_exit_vetoes_without_descendant_kill_guarantee() {
    let attempts = std::cell::Cell::new(0);
    let prevented = std::cell::Cell::new(false);
    let outcome = handle_exit_request(
        || {
            attempts.set(attempts.get() + 1);
            Err(crate::acp::runtime::HostError::Deadline)
        },
        || prevented.set(true),
        true,
    );
    assert_eq!(attempts.get(), 1);
    assert!(prevented.get());
    assert_eq!(
        outcome,
        ExitRequestOutcome::Deferred(crate::acp::runtime::HostError::Deadline)
    );
}
