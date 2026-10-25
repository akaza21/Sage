use crate::acp::browser::BrowserFailure;

pub(in crate::browser::devtools) fn dispatch_timeout_failure(issued: bool) -> BrowserFailure {
    if issued {
        BrowserFailure::OutcomeUnknown
    } else {
        BrowserFailure::Unavailable
    }
}

pub(in crate::browser::devtools) fn dispatch_disconnected_failure(issued: bool) -> BrowserFailure {
    dispatch_timeout_failure(issued)
}

pub(super) fn navigation_failure(issued: bool, error: BrowserFailure) -> BrowserFailure {
    if issued {
        BrowserFailure::OutcomeUnknown
    } else {
        error
    }
}

pub(super) fn navigation_matches(
    requested_url: &str,
    issued_loader_id: Option<&str>,
    settled_url: &str,
    settled_loader_id: Option<&str>,
) -> bool {
    match issued_loader_id {
        Some(issued) => settled_loader_id == Some(issued),
        None => settled_url == requested_url,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outer_timeout_preserves_unknown_outcome_after_issue() {
        assert_eq!(
            dispatch_timeout_failure(true),
            BrowserFailure::OutcomeUnknown
        );
        assert_eq!(dispatch_timeout_failure(false), BrowserFailure::Unavailable);
    }

    #[test]
    fn disconnected_worker_after_issue_has_unknown_outcome() {
        assert_eq!(
            dispatch_disconnected_failure(false),
            BrowserFailure::Unavailable
        );
        assert_eq!(
            dispatch_disconnected_failure(true),
            BrowserFailure::OutcomeUnknown
        );
    }

    #[test]
    fn navigation_failures_after_issue_are_unknown_and_unrelated_loads_never_match() {
        assert_eq!(
            navigation_failure(false, BrowserFailure::Unavailable),
            BrowserFailure::Unavailable
        );
        assert_eq!(
            navigation_failure(true, BrowserFailure::Unavailable),
            BrowserFailure::OutcomeUnknown
        );
        assert!(!navigation_matches(
            "https://site.test/target",
            Some("loader-1"),
            "https://site.test/elsewhere",
            Some("loader-2")
        ));
        assert!(navigation_matches(
            "https://site.test/target",
            Some("loader-1"),
            "https://site.test/redirected",
            Some("loader-1")
        ));
        assert!(!navigation_matches(
            "https://site.test/target#section",
            None,
            "https://site.test/elsewhere",
            None
        ));
        assert!(navigation_matches(
            "https://site.test/target#section",
            None,
            "https://site.test/target#section",
            None
        ));
    }
}
