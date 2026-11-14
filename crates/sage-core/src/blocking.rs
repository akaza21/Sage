//! Synchronous work called from async code.

use tokio::runtime::{Handle, RuntimeFlavor};

/// Runs `work` in place, first handing this thread's other tasks to the rest
/// of a multi-thread runtime so blocking work (SQLite, process waits) does
/// not stall them. On a current-thread runtime, or outside any runtime, it
/// just runs `work`, since `block_in_place` would panic there.
pub fn in_place<R>(work: impl FnOnce() -> R) -> R {
    if Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(work)
    } else {
        work()
    }
}
