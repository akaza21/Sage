use std::time::Instant;
use std::{
    future::Future,
    sync::{Mutex, MutexGuard, PoisonError},
    time::Duration,
};

pub const HANG_GUARD: Duration = Duration::from_secs(120);
pub const BUDGET: Duration = Duration::from_secs(60);

static SERIAL_PROCESS_TESTS: Mutex<()> = Mutex::new(());

pub fn serial_process_guard() -> MutexGuard<'static, ()> {
    SERIAL_PROCESS_TESTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
}

#[track_caller]
pub fn guarded<F>(label: &str, future: F) -> F::Output
where
    F: Future,
{
    tokio::runtime::Runtime::new()
        .expect("guarded test runtime")
        .block_on(async move {
            match tokio::time::timeout(HANG_GUARD, future).await {
                Ok(output) => output,
                Err(_) => {
                    panic!("{label} did not finish within the {HANG_GUARD:?} test hang guard")
                }
            }
        })
}

#[track_caller]
pub fn until(label: &str, mut condition: impl FnMut() -> bool) {
    let deadline = Instant::now() + HANG_GUARD;
    while !condition() {
        assert!(
            Instant::now() < deadline,
            "{label} did not converge within the {HANG_GUARD:?} test hang guard"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}
