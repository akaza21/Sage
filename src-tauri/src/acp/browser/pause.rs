//! Take over pauses the Agent's browser call; Hand back resumes it. A call
//! refused, before it was issued, because the user holds a tab of its chat
//! waits, as a permission card does, for the user to hand the tab back, and
//! then runs once more under a fresh authority, so it asks permission again.
//! Only a hand back resumes it: Stop or a detach that leaves no tab held
//! ends the pause with the call's own failure, and the turn ending, the
//! owner moving on or the wait running out end it with `UserControl`.

#![cfg_attr(not(windows), allow(dead_code))]

use std::time::{Duration, Instant};

use super::BrowserFailure;

/// As long as a permission card waits for an answer.
pub(crate) const HAND_BACK_WAIT: Duration = Duration::from_secs(300);
pub(crate) const POLL: Duration = Duration::from_millis(100);

/// What the pause reads from the attachments and the turn.
pub(crate) struct PauseSignals<Holds, Generation, Current> {
    /// Whether the user holds any tab of the call's chat.
    pub(crate) user_holds: Holds,
    /// Moves on each hand back.
    pub(crate) hand_backs: Generation,
    /// Whether the owner the call started under is still current and its
    /// turn runs.
    pub(crate) owner_current: Current,
}

/// Whether `outcome` is a call the user's take over paused before it was
/// issued, rather than one refused for any other reason. `UserControl` is
/// returned only before issue. `Denied` counts only when the user holds the
/// very tab the call targeted (`declined_by_take_over`): Take over then
/// declined its permission card or ended its authority before issue. A
/// Deny on one tab while the user holds another is the user's answer.
pub(crate) fn paused<T>(outcome: &Result<T, BrowserFailure>, declined_by_take_over: bool) -> bool {
    match outcome {
        Err(BrowserFailure::UserControl) => true,
        Err(BrowserFailure::Denied) => declined_by_take_over,
        _ => false,
    }
}

/// Runs `attempt`, and when `is_paused` says the user's take over refused
/// it, waits for a hand back and runs it once more.
pub(crate) fn run_with_pause<T, Holds, Generation, Current>(
    mut attempt: impl FnMut() -> Result<T, BrowserFailure>,
    is_paused: impl FnOnce(&Result<T, BrowserFailure>) -> bool,
    signals: PauseSignals<Holds, Generation, Current>,
    wait: Duration,
    poll: Duration,
) -> Result<T, BrowserFailure>
where
    Holds: Fn() -> bool,
    Generation: Fn() -> u64,
    Current: Fn() -> bool,
{
    let generation = (signals.hand_backs)();
    let first = attempt();
    if !is_paused(&first) || !(signals.owner_current)() {
        return first;
    }
    let deadline = Instant::now() + wait;
    loop {
        if !(signals.owner_current)() {
            return Err(BrowserFailure::UserControl);
        }
        if !(signals.user_holds)() {
            if (signals.hand_backs)() != generation {
                return attempt();
            }
            // Stop or a detach: nothing was handed back, nothing resumes.
            return match first {
                Err(BrowserFailure::UserControl) => Err(BrowserFailure::Denied),
                other => other,
            };
        }
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(BrowserFailure::UserControl);
        }
        std::thread::sleep(poll.min(left));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    const WAIT: Duration = Duration::from_secs(5);
    const TICK: Duration = Duration::from_millis(1);

    /// A chat whose user holds a tab for `polls` reads, then lets go, and
    /// hands back when `hand_back` is set.
    struct Chat {
        holds_for: Cell<u32>,
        hand_back: bool,
        generation: Cell<u64>,
        attempts: Cell<u32>,
    }

    impl Chat {
        fn new(holds_for: u32, hand_back: bool) -> Self {
            Self {
                holds_for: Cell::new(holds_for),
                hand_back,
                generation: Cell::new(0),
                attempts: Cell::new(0),
            }
        }

        fn holds(&self) -> bool {
            let left = self.holds_for.get();
            if left == 0 {
                return false;
            }
            self.holds_for.set(left - 1);
            if left == 1 && self.hand_back {
                self.generation.set(self.generation.get() + 1);
            }
            true
        }

        fn run(
            &self,
            first: BrowserFailure,
            current: impl Fn() -> bool,
        ) -> Result<u32, BrowserFailure> {
            run_with_pause(
                || {
                    self.attempts.set(self.attempts.get() + 1);
                    if self.attempts.get() == 1 {
                        Err(first)
                    } else {
                        Ok(self.attempts.get())
                    }
                },
                |outcome| paused(outcome, true),
                PauseSignals {
                    user_holds: || self.holds(),
                    hand_backs: || self.generation.get(),
                    owner_current: current,
                },
                WAIT,
                TICK,
            )
        }
    }

    #[test]
    fn hand_back_resumes_the_paused_call_once() {
        let chat = Chat::new(3, true);
        assert_eq!(chat.run(BrowserFailure::UserControl, || true), Ok(2));
        assert_eq!(chat.attempts.get(), 2);
    }

    #[test]
    fn stop_while_paused_does_not_resume() {
        // The user stops the held tab: no tab is held, nothing was handed back.
        let chat = Chat::new(3, false);
        assert_eq!(
            chat.run(BrowserFailure::UserControl, || true),
            Err(BrowserFailure::Denied)
        );
        assert_eq!(chat.attempts.get(), 1);
        let chat = Chat::new(3, false);
        assert_eq!(
            chat.run(BrowserFailure::Denied, || true),
            Err(BrowserFailure::Denied)
        );
        assert_eq!(chat.attempts.get(), 1);
    }

    #[test]
    fn a_new_turn_or_the_deadline_ends_the_pause() {
        let chat = Chat::new(u32::MAX, true);
        let reads = Cell::new(0);
        let ended = chat.run(BrowserFailure::UserControl, || {
            reads.set(reads.get() + 1);
            reads.get() < 3
        });
        assert_eq!(
            ended,
            Err(BrowserFailure::UserControl),
            "the turn ended while paused"
        );
        assert_eq!(chat.attempts.get(), 1);

        let started = Instant::now();
        let generation = Cell::new(0);
        let ran_out = run_with_pause::<(), _, _, _>(
            || Err(BrowserFailure::UserControl),
            |outcome| paused(outcome, false),
            PauseSignals {
                user_holds: || true,
                hand_backs: || generation.get(),
                owner_current: || true,
            },
            Duration::from_millis(20),
            Duration::from_millis(5),
        );
        assert_eq!(ran_out, Err(BrowserFailure::UserControl));
        assert!(started.elapsed() < Duration::from_secs(1));
    }

    #[test]
    fn no_pause_without_a_current_owner_or_for_other_failures() {
        let chat = Chat::new(3, true);
        assert_eq!(
            chat.run(BrowserFailure::UserControl, || false),
            Err(BrowserFailure::UserControl)
        );
        assert_eq!(chat.attempts.get(), 1);
        let chat = Chat::new(3, true);
        assert_eq!(
            chat.run(BrowserFailure::OutcomeUnknown, || true),
            Err(BrowserFailure::OutcomeUnknown)
        );
        assert_eq!(chat.attempts.get(), 1);
    }

    #[test]
    fn only_a_take_over_counts_as_a_pause() {
        assert!(paused::<()>(&Err(BrowserFailure::UserControl), false));
        assert!(paused::<()>(&Err(BrowserFailure::Denied), true));
        // An explicit Deny on one tab while the user holds another tab.
        assert!(!paused::<()>(&Err(BrowserFailure::Denied), false));
        assert!(!paused::<()>(&Err(BrowserFailure::OutcomeUnknown), true));
        assert!(!paused(&Ok(()), true));
    }

    #[test]
    fn a_hand_back_of_another_tab_waits_for_the_held_one() {
        // A hand back moved the generation, but a tab is still held.
        let holds = Cell::new(4_u32);
        let generation = Cell::new(0);
        let attempts = Cell::new(0);
        let result = run_with_pause(
            || {
                attempts.set(attempts.get() + 1);
                if attempts.get() == 1 {
                    generation.set(1);
                    Err(BrowserFailure::UserControl)
                } else {
                    Ok(())
                }
            },
            |outcome| paused(outcome, false),
            PauseSignals {
                user_holds: || {
                    holds.set(holds.get().saturating_sub(1));
                    holds.get() > 0
                },
                hand_backs: || generation.get(),
                owner_current: || true,
            },
            WAIT,
            TICK,
        );
        assert_eq!(result, Ok(()));
        assert_eq!(holds.get(), 0, "resumed only once nothing was held");
    }
}
