use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use windows_sys::Win32::System::Threading::{GetCurrentProcess, TerminateProcess};

use super::protocol::BoundaryError;

pub(crate) const HELPER_OPERATION_DEADLINE: Duration = Duration::from_secs(25);
const WATCHDOG_EXIT_CODE: u32 = 18;

pub(crate) fn run_with_watchdog<T>(
    timeout: Duration,
    terminate: impl FnOnce() + Send + 'static,
    operation: impl FnOnce() -> Result<T, BoundaryError>,
) -> Result<T, BoundaryError> {
    if timeout.is_zero() {
        return Err(BoundaryError::Deadline);
    }
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or(BoundaryError::Deadline)?;
    let watchdog = OperationWatchdog::start(deadline, terminate)?;
    let result = operation();
    watchdog.disarm_and_join()?;
    result
}

pub(crate) fn terminate_current_process() {
    unsafe {
        TerminateProcess(GetCurrentProcess(), WATCHDOG_EXIT_CODE);
    }
    // TerminateProcess can report failure. Aborting is the no-return fallback.
    std::process::abort();
}

struct OperationWatchdog {
    disarm: Option<mpsc::SyncSender<Instant>>,
    thread: Option<JoinHandle<()>>,
    expired: Arc<AtomicBool>,
}

impl OperationWatchdog {
    fn start(
        deadline: Instant,
        terminate: impl FnOnce() + Send + 'static,
    ) -> Result<Self, BoundaryError> {
        let (disarm, wait_for_disarm) = mpsc::sync_channel(1);
        let expired = Arc::new(AtomicBool::new(false));
        let observed_expiry = Arc::clone(&expired);
        let thread = std::thread::Builder::new()
            .name("sage-codex-helper-watchdog".to_owned())
            .spawn(move || {
                let remaining = deadline.saturating_duration_since(Instant::now());
                match wait_for_disarm.recv_timeout(remaining) {
                    Ok(completed_at) if completed_at < deadline => return,
                    Err(mpsc::RecvTimeoutError::Disconnected) => return,
                    Ok(_) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                observed_expiry.store(true, Ordering::Release);
                terminate();
            })
            .map_err(|_| BoundaryError::Io)?;
        Ok(Self {
            disarm: Some(disarm),
            thread: Some(thread),
            expired,
        })
    }

    fn disarm_and_join(mut self) -> Result<(), BoundaryError> {
        self.stop()
    }

    fn stop(&mut self) -> Result<(), BoundaryError> {
        if let Some(disarm) = self.disarm.take() {
            let _ = disarm.try_send(Instant::now());
        }
        if let Some(thread) = self.thread.take() {
            thread.join().map_err(|_| BoundaryError::Io)?;
        }
        if self.expired.load(Ordering::Acquire) {
            return Err(BoundaryError::Deadline);
        }
        Ok(())
    }
}

impl Drop for OperationWatchdog {
    fn drop(&mut self) {
        let _ = self.stop();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, mpsc};

    use super::*;

    struct DropProbe(Arc<AtomicUsize>);

    impl Drop for DropProbe {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[test]
    fn expired_watchdog_invokes_termination_and_fails_closed() {
        let termination_calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = Arc::clone(&termination_calls);
        let (terminated, wait_for_termination) = mpsc::sync_channel(1);

        let result = run_with_watchdog(
            Duration::from_millis(1),
            move || {
                observed_calls.fetch_add(1, Ordering::SeqCst);
                terminated.send(()).unwrap();
            },
            move || {
                wait_for_termination
                    .recv_timeout(Duration::from_secs(1))
                    .unwrap();
                Ok(())
            },
        );

        assert_eq!(result, Err(BoundaryError::Deadline));
        assert_eq!(termination_calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn successful_operation_disarms_and_joins_without_late_termination() {
        let termination_calls = Arc::new(AtomicUsize::new(0));
        let observed_calls = Arc::clone(&termination_calls);
        let retired_terminator = Arc::new(AtomicUsize::new(0));
        let probe = DropProbe(Arc::clone(&retired_terminator));

        let result = run_with_watchdog(
            Duration::from_secs(1),
            move || {
                let _probe = probe;
                observed_calls.fetch_add(1, Ordering::SeqCst);
            },
            || Ok(41),
        );

        assert_eq!(result, Ok(41));
        assert_eq!(termination_calls.load(Ordering::SeqCst), 0);
        assert_eq!(retired_terminator.load(Ordering::SeqCst), 1);
    }
}
