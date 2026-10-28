use super::BrowserFailure;

fn close_with(
    failure: BrowserFailure,
    close: impl FnOnce() -> Result<(), BrowserFailure>,
) -> Result<(), BrowserFailure> {
    close().map_err(|_| BrowserFailure::OutcomeUnknown)?;
    Err(failure)
}

fn close_attached(
    failure: BrowserFailure,
    close: impl FnOnce() -> Result<(), BrowserFailure>,
    detach: impl FnOnce(),
    retire: impl FnOnce(),
) -> Result<(), BrowserFailure> {
    if close().is_ok() {
        detach();
    } else {
        retire();
    }
    Err(failure)
}

/// `owner_current` answers why the tab may no longer be committed: `Denied`
/// when the owner moved on, `UserControl` when the user took over a tab of
/// this chat while the tab opened.
pub(super) fn complete_opened_tab(
    owner_current: impl Fn() -> Result<(), BrowserFailure>,
    attach: impl FnOnce() -> Result<(), BrowserFailure>,
    detach: impl FnOnce(),
    retire: impl FnOnce(),
    emit: impl FnOnce() -> Result<(), BrowserFailure>,
    close: impl FnOnce() -> Result<(), BrowserFailure>,
) -> Result<(), BrowserFailure> {
    if let Err(failure) = owner_current() {
        return close_with(failure, close);
    }
    if let Err(failure) = attach() {
        return close_with(failure, close);
    }
    if let Err(failure) = owner_current() {
        return close_attached(failure, close, detach, retire);
    }
    if let Err(failure) = emit() {
        return close_attached(failure, close, detach, retire);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn owner_rotation_during_attach_leaves_no_lease_event_or_tab() {
        let owner_live = Cell::new(true);
        let lease = Cell::new(false);
        let event = Cell::new(false);
        let tab = Cell::new(true);
        let result = complete_opened_tab(
            || {
                if owner_live.get() {
                    Ok(())
                } else {
                    Err(BrowserFailure::Denied)
                }
            },
            || {
                lease.set(true);
                owner_live.set(false);
                Ok(())
            },
            || lease.set(false),
            || lease.set(false),
            || {
                event.set(true);
                Ok(())
            },
            || {
                tab.set(false);
                Ok(())
            },
        );
        assert_eq!(result, Err(BrowserFailure::Denied));
        assert!(!lease.get());
        assert!(!event.get());
        assert!(!tab.get());
    }

    fn close_failure_after_owner_rotation_keeps_tab_tracked(close_failure: BrowserFailure) {
        let owner_live = Cell::new(true);
        let lease = Cell::new(false);
        let retired = Cell::new(false);
        let event = Cell::new(false);
        let tab = Cell::new(true);
        let result = complete_opened_tab(
            || {
                if owner_live.get() {
                    Ok(())
                } else {
                    Err(BrowserFailure::Denied)
                }
            },
            || {
                lease.set(true);
                owner_live.set(false);
                Ok(())
            },
            || lease.set(false),
            || retired.set(true),
            || {
                event.set(true);
                Ok(())
            },
            || Err(close_failure),
        );
        assert_eq!(result, Err(BrowserFailure::Denied));
        assert!(lease.get());
        assert!(retired.get());
        assert!(tab.get());
        assert!(!event.get());
    }

    #[test]
    fn a_take_over_while_the_tab_opens_closes_it_and_reports_user_control() {
        let taken_over = Cell::new(false);
        let lease = Cell::new(false);
        let event = Cell::new(false);
        let tab = Cell::new(true);
        let result = complete_opened_tab(
            || {
                if taken_over.get() {
                    Err(BrowserFailure::UserControl)
                } else {
                    Ok(())
                }
            },
            || {
                lease.set(true);
                taken_over.set(true);
                Ok(())
            },
            || lease.set(false),
            || lease.set(false),
            || {
                event.set(true);
                Ok(())
            },
            || {
                tab.set(false);
                Ok(())
            },
        );
        assert_eq!(result, Err(BrowserFailure::UserControl));
        assert!(!lease.get());
        assert!(!event.get());
        assert!(!tab.get());
    }

    #[test]
    fn definite_close_failure_after_owner_rotation_keeps_tab_tracked_for_sweep() {
        close_failure_after_owner_rotation_keeps_tab_tracked(BrowserFailure::Unavailable);
    }

    #[test]
    fn uncertain_close_failure_after_owner_rotation_keeps_tab_tracked_for_sweep() {
        close_failure_after_owner_rotation_keeps_tab_tracked(BrowserFailure::OutcomeUnknown);
    }
}
