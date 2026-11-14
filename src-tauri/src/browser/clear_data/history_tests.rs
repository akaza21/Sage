//! How a browsing-history failure shapes a clear's result.

use super::*;

#[test]
fn a_history_failure_turns_a_clear_into_a_failure_and_keeps_the_first_error() {
    let mut result = ClearBrowsingDataResult {
        closed_tab_ids: Vec::new(),
        cleared: true,
        pending: false,
        error: None,
    };
    append_error(&mut result, None);
    assert!(result.cleared);
    result.error = Some("Cookies were not cleared.".into());
    append_error(&mut result, Some("History was not cleared.".into()));
    assert!(!result.cleared);
    assert_eq!(
        result.error.as_deref(),
        Some("Cookies were not cleared. History was not cleared.")
    );
}

#[cfg(windows)]
#[test]
fn a_pending_clear_fails_when_history_was_not_cleared() {
    const HISTORY: &str = "Browsing history was not cleared: busy";
    assert_eq!(pending_outcome(Ok(()), None), Ok(()));
    assert_eq!(
        pending_outcome(Ok(()), Some(HISTORY.into())),
        Err(HISTORY.to_owned())
    );
    assert_eq!(
        pending_outcome(Err("profile".into()), Some(HISTORY.into())),
        Err("profile".to_owned())
    );
}
