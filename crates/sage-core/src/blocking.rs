use tokio::runtime::{Handle, RuntimeFlavor};

/// On a current-thread runtime, or outside any runtime, `block_in_place` would
/// panic, so `work` just runs there.
pub fn in_place<R>(work: impl FnOnce() -> R) -> R {
    if Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(work)
    } else {
        work()
    }
}
