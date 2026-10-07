use super::*;
use std::sync::atomic::AtomicBool;

#[test]
fn delayed_ui_callback_does_not_issue_after_timeout() {
    let attempt = IssueAttempt::new();
    let issued = AtomicBool::new(false);
    assert_eq!(attempt.timeout(), BrowserFailure::Unavailable);
    assert_eq!(
        attempt.issue(None, || {
            issued.store(true, Ordering::SeqCst);
            Ok(())
        }),
        Err(BrowserFailure::Unavailable)
    );
    assert!(!issued.load(Ordering::SeqCst));
}

#[test]
fn issued_command_with_no_result_has_unknown_outcome() {
    let attempt = IssueAttempt::new();
    assert_eq!(attempt.issue(None, || Ok(())), Ok(()));
    assert_eq!(attempt.timeout(), BrowserFailure::OutcomeUnknown);
}

#[test]
fn disconnected_completion_after_issue_has_unknown_outcome() {
    let attempt = IssueAttempt::new();
    assert_eq!(attempt.disconnected_failure(), BrowserFailure::Unavailable);
    let attempt = IssueAttempt::new();
    assert_eq!(attempt.issue(None, || Ok(())), Ok(()));
    assert_eq!(
        attempt.disconnected_failure(),
        BrowserFailure::OutcomeUnknown
    );
}
