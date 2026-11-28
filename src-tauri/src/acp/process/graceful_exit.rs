//! How Sage ends a harness it no longer needs. A harness may still be saving
//! work when Sage lets it go: Claude Code, for one, refreshes an expiring
//! OAuth sign-in during a turn, and the sign-in server retires the old refresh
//! token as soon as it answers. On a clean exit the harness waits for that
//! refresh and saves the new token; a process killed in between loses it, and
//! the saved sign-in can no longer be refreshed on any machine that shares it.
//! So every intentional close ends the harness's input first, leaves it
//! [`EXIT_GRACE`] to exit on its own, and only then kills it.

use std::{
    process::Child,
    thread,
    time::{Duration, Instant},
};

/// Longer than Claude Code's own wait (about 8.5 s) for an in-flight refresh
/// on exit.
pub(crate) const EXIT_GRACE: Duration = Duration::from_secs(10);
const EXIT_POLL: Duration = Duration::from_millis(50);

/// The longest an intentional close of a harness may take: the grace it is
/// left to exit on its own, then the explicit-close budget for the kill and
/// the tree to reach zero.
pub(crate) fn close_budget() -> Duration {
    EXIT_GRACE + Duration::from_millis(sage_core::limits::LIMITS.deadlines_ms.explicit_close)
}

pub(crate) fn close_budget_ms() -> u64 {
    u64::try_from(close_budget().as_millis()).unwrap_or(u64::MAX)
}

/// The grace a harness can still have when its teardown must end by
/// `deadline`: at most [`EXIT_GRACE`], leaving the tree-termination budget to
/// kill whatever is left and reach zero.
pub(crate) fn grace_before(deadline: Instant) -> Duration {
    deadline
        .saturating_duration_since(Instant::now())
        .saturating_sub(Duration::from_millis(
            sage_core::limits::LIMITS.deadlines_ms.tree_termination,
        ))
        .min(EXIT_GRACE)
}

/// Ends a supervised harness whose stdin the caller has closed after work
/// that finished cleanly: it is left the grace that fits before `deadline`
/// to exit on its own, then the tree is killed, and zero is proven by
/// `deadline`.
pub(crate) fn end_tree_before(
    control: &super::TreeControl,
    reason: super::TreeExit,
    deadline: Instant,
) -> Result<(super::TreeExit, super::ZeroProcessToken), sage_process::error::LaunchError> {
    // An absolute end no later than `deadline`, so the wait below always
    // claims the grace and kills the tree, however late this thread runs.
    let kill_at = (Instant::now() + grace_before(deadline)).min(deadline);
    control.request_graceful_termination_until(reason, kill_at)?;
    control.wait_zero_before(deadline)
}

/// A process whose input Sage has already ended.
pub(crate) trait ExitingProcess {
    /// True once it has exited, or when its state can no longer be read.
    fn exited(&mut self) -> bool;
    /// Ends it, with every process it owns where the platform tracks them.
    fn kill(&mut self);
}

/// Waits until `deadline` for `process` to exit on its own and kills it if it
/// has not. True when it exited on its own.
pub(crate) fn exit_or_kill(process: &mut impl ExitingProcess, deadline: Instant) -> bool {
    loop {
        if process.exited() {
            return true;
        }
        let now = Instant::now();
        if now >= deadline {
            break;
        }
        thread::sleep(EXIT_POLL.min(deadline - now));
    }
    process.kill();
    false
}

/// Runs `close` on every item at once, so each harness's grace runs alongside
/// the others' and closing them all takes about as long as the slowest one.
/// Callers pass at most one item per live process tree, which
/// `LIMITS.process.operational_live_trees` bounds. An item whose thread cannot
/// start is closed on the calling thread afterwards.
pub(crate) fn close_all<T, R>(items: &[T], close: impl Fn(&T) -> R + Sync) -> Vec<R>
where
    T: Sync,
    R: Send,
{
    thread::scope(|scope| {
        let started: Vec<_> = items
            .iter()
            .map(|item| {
                thread::Builder::new()
                    .name("sage-harness-close".into())
                    .spawn_scoped(scope, || close(item))
            })
            .collect();
        started
            .into_iter()
            .zip(items)
            .map(|(thread, item)| match thread {
                Ok(thread) => thread
                    .join()
                    .unwrap_or_else(|panic| std::panic::resume_unwind(panic)),
                Err(_) => close(item),
            })
            .collect()
    })
}

impl ExitingProcess for Child {
    fn exited(&mut self) -> bool {
        !matches!(self.try_wait(), Ok(None))
    }

    fn kill(&mut self) {
        let _ = Child::kill(self);
        let _ = self.wait();
    }
}

#[cfg(test)]
#[path = "graceful_exit_tests.rs"]
mod tests;
