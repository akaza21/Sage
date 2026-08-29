use std::{
    panic::{AssertUnwindSafe, catch_unwind},
    sync::mpsc,
    time::Duration,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ThreadWaitError {
    Timeout,
    Panicked,
}

pub(crate) struct BoundedThread {
    handle: Option<std::thread::JoinHandle<()>>,
    completion: mpsc::Receiver<bool>,
}

impl BoundedThread {
    pub(crate) fn spawn(
        name: impl Into<String>,
        task: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        Self::spawn_with_stack(name, None, task)
    }

    pub(crate) fn spawn_with_stack(
        name: impl Into<String>,
        stack_size: Option<usize>,
        task: impl FnOnce() + Send + 'static,
    ) -> std::io::Result<Self> {
        let (completed, completion) = mpsc::sync_channel(1);
        let mut builder = std::thread::Builder::new();
        builder = builder.name(name.into());
        if let Some(stack_size) = stack_size {
            builder = builder.stack_size(stack_size);
        }
        let handle = builder.spawn(move || {
            let clean = catch_unwind(AssertUnwindSafe(task)).is_ok();
            let _ = completed.send(clean);
        })?;
        Ok(Self {
            handle: Some(handle),
            completion,
        })
    }

    /// Waits only for the worker's explicit completion signal. The join handle
    /// is dropped after that signal rather than joined, so thread teardown can
    /// never extend the caller's frozen deadline through a blocking destructor.
    pub(crate) fn wait(&mut self, timeout: Duration) -> Result<(), ThreadWaitError> {
        if self.handle.is_none() {
            return Ok(());
        }
        let clean = self
            .completion
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => ThreadWaitError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => ThreadWaitError::Panicked,
            })?;
        drop(self.handle.take());
        if clean {
            Ok(())
        } else {
            Err(ThreadWaitError::Panicked)
        }
    }

    #[cfg(test)]
    pub(crate) fn wait_and_join(&mut self, timeout: Duration) -> Result<(), ThreadWaitError> {
        if self.handle.is_none() {
            return Ok(());
        }
        let started = std::time::Instant::now();
        let clean = self
            .completion
            .recv_timeout(timeout)
            .map_err(|error| match error {
                mpsc::RecvTimeoutError::Timeout => ThreadWaitError::Timeout,
                mpsc::RecvTimeoutError::Disconnected => ThreadWaitError::Panicked,
            })?;

        // The completion message is emitted at the end of the worker body, but
        // a destructor can still run before the thread itself exits. Observe
        // that exit within the same deadline before joining. Once
        // `is_finished` is true, `join` cannot wait on a running thread.
        while !self
            .handle
            .as_ref()
            .expect("bounded thread handle retained")
            .is_finished()
        {
            let remaining = timeout.saturating_sub(started.elapsed());
            if remaining.is_zero() {
                return Err(ThreadWaitError::Timeout);
            }
            std::thread::park_timeout(remaining.min(Duration::from_millis(1)));
        }
        let joined = self
            .handle
            .take()
            .expect("bounded thread handle retained")
            .join()
            .is_ok();
        if clean && joined {
            Ok(())
        } else {
            Err(ThreadWaitError::Panicked)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{Arc, Condvar, Mutex},
        time::{Duration, Instant},
    };

    use super::*;

    #[test]
    fn stalled_thread_wait_returns_at_the_caller_deadline() {
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let worker_gate = Arc::clone(&gate);
        let mut thread = BoundedThread::spawn("bounded-thread-test", move || {
            let (state, ready) = &*worker_gate;
            let mut released = state.lock().unwrap();
            while !*released {
                released = ready.wait(released).unwrap();
            }
        })
        .unwrap();

        let started = Instant::now();
        assert_eq!(
            thread.wait(Duration::from_millis(20)),
            Err(ThreadWaitError::Timeout)
        );
        assert!(started.elapsed() < Duration::from_secs(1));

        let (state, ready) = &*gate;
        *state.lock().unwrap() = true;
        ready.notify_all();
        assert_eq!(thread.wait(Duration::from_secs(1)), Ok(()));
    }

    #[test]
    fn wait_and_join_observes_thread_exit_inside_the_caller_deadline() {
        let mut thread = BoundedThread::spawn("bounded-thread-join-test", || {}).unwrap();

        assert_eq!(thread.wait_and_join(Duration::from_secs(1)), Ok(()));
        assert!(thread.handle.is_none());
    }
}
