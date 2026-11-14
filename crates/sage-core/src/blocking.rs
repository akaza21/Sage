use tokio::runtime::{Handle, RuntimeFlavor};

/// `block_in_place` panics on a current-thread runtime, so `work` runs directly
/// there and outside any runtime.
pub fn in_place<R>(work: impl FnOnce() -> R) -> R {
    if Handle::try_current()
        .is_ok_and(|handle| handle.runtime_flavor() == RuntimeFlavor::MultiThread)
    {
        tokio::task::block_in_place(work)
    } else {
        work()
    }
}
