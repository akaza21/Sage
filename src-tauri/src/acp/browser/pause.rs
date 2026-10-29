//! Take over pauses the Agent's browser call; Hand back resumes it. A call
//! refused because the user holds a tab of its chat waits, as a permission
//! card does, for the user to hand the tab back, and then runs again under a
//! fresh authority, so it asks permission again. The turn ending, the owner
//! moving on or the wait running out ends the pause with `UserControl`.

#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

use super::BrowserFailure;

/// As long as a permission card waits for an answer.
pub(crate) const HAND_BACK_WAIT: Duration = Duration::from_secs(300);
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// Whether `outcome` is a call the user's take over paused, rather than one
/// refused for any other reason. `Denied` counts only while the user still
/// holds the chat's tab: Take over declined its permission card before the
/// call was issued, so running it again is safe.
pub(crate) fn paused<T>(
    outcome: &Result<T, BrowserFailure>,
    user_holds: impl Fn() -> bool,
) -> bool {
    match outcome {
        Err(BrowserFailure::UserControl) => true,
        Err(BrowserFailure::Denied) => user_holds(),
        _ => false,
    }
}

/// Waits until the user hands back. True when the call may run again: the
/// user no longer holds any of the chat's tabs and the owner is current.
pub(crate) fn wait_for_hand_back(
    user_holds: impl Fn() -> bool,
    owner_current: impl Fn() -> bool,
    deadline: Instant,
    poll: Duration,
) -> bool {
    loop {
        if !owner_current() {
            return false;
        }
        if !user_holds() {
            return true;
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return false;
        }
        std::thread::sleep(poll.min(left));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn hand_back_resumes_the_paused_call() {
        let polls = Cell::new(0);
        let resumed = wait_for_hand_back(
            || {
                polls.set(polls.get() + 1);
                polls.get() < 3
            },
            || true,
            Instant::now() + Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert!(resumed);
        assert_eq!(polls.get(), 3);
    }

    #[test]
    fn a_new_turn_or_the_deadline_ends_the_pause() {
        let current = Cell::new(true);
        let ended = wait_for_hand_back(
            || {
                current.set(false);
                true
            },
            || current.get(),
            Instant::now() + Duration::from_secs(5),
            Duration::from_millis(1),
        );
        assert!(!ended, "the turn ended while paused");
        let started = Instant::now();
        assert!(!wait_for_hand_back(
            || true,
            || true,
            Instant::now() + Duration::from_millis(20),
            Duration::from_millis(5),
        ));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn only_a_take_over_counts_as_a_pause() {
        let held = || true;
        let free = || false;
        assert!(paused::<()>(&Err(BrowserFailure::UserControl), free));
        assert!(paused::<()>(&Err(BrowserFailure::Denied), held));
        assert!(!paused::<()>(&Err(BrowserFailure::Denied), free));
        assert!(!paused::<()>(&Err(BrowserFailure::OutcomeUnknown), held));
        assert!(!paused(&Ok(()), held));
    }
}
