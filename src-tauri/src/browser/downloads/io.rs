//! Destination I/O never runs on the coordinator or UI thread. Each folder has
//! one sleeping worker and a bounded queue. A lease is admitted before planning
//! and retained through publication/cleanup, reserving room for disposal even
//! when a native call never returns. Expiry never releases a stuck call's slot.

use std::{
    io,
    panic::{AssertUnwindSafe, catch_unwind},
    path::Path,
    sync::{
        Arc, Mutex, OnceLock, Weak,
        atomic::{AtomicUsize, Ordering},
        mpsc::{self, SyncSender},
    },
    time::{Duration, Instant},
};

const MAX_PER_DESTINATION: usize = super::MAX_ACTIVE_DOWNLOADS;
// One stalled folder cannot consume the capacity reserved for another folder.
const MAX_LEASES: usize = 2 * MAX_PER_DESTINATION;
const TIMEOUT: Duration = Duration::from_secs(2);
const STACK_BYTES: usize = 256 * 1024;
type Job = Box<dyn FnOnce() + Send>;

#[derive(Clone, Copy)]
pub(crate) struct Deadline(Instant);

impl Deadline {
    pub(crate) fn check(self) -> io::Result<()> {
        if Instant::now() >= self.0 {
            Err(timed_out())
        } else {
            Ok(())
        }
    }
}

fn timed_out() -> io::Error {
    io::Error::new(
        io::ErrorKind::TimedOut,
        "download destination I/O timed out",
    )
}

#[derive(Debug)]
struct Lane {
    sender: SyncSender<Job>,
    active: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct Permit {
    total: Arc<AtomicUsize>,
    destination: Arc<AtomicUsize>,
}

impl Drop for Permit {
    fn drop(&mut self) {
        self.total.fetch_sub(1, Ordering::AcqRel);
        self.destination.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug)]
pub(crate) struct Destination {
    lane: Arc<Lane>,
    permit: Arc<Permit>,
    timeout: Duration,
}

pub(crate) struct Executor {
    lanes: Mutex<Vec<(String, Weak<Lane>)>>,
    active: Arc<AtomicUsize>,
    workers: Arc<AtomicUsize>,
    timeout: Duration,
}

impl Executor {
    pub(crate) fn new(timeout: Duration) -> Self {
        Self {
            lanes: Mutex::new(Vec::new()),
            active: Arc::new(AtomicUsize::new(0)),
            workers: Arc::new(AtomicUsize::new(0)),
            timeout,
        }
    }

    pub(crate) fn acquire(&self, folder: &Path) -> Option<Destination> {
        // Settings supplies canonical paths. Never resolve a path here: even
        // canonicalization can block. Keep that original lane through handoff.
        let key = folder.to_string_lossy().to_lowercase();
        let mut lanes = self.lanes.lock().ok()?;
        lanes.retain(|(_, lane)| lane.strong_count() != 0);
        if self.active.load(Ordering::Acquire) >= MAX_LEASES {
            return None;
        }
        let existing = lanes
            .iter()
            .find(|(path, _)| *path == key)
            .and_then(|(_, lane)| lane.upgrade());
        let lane = match existing {
            Some(lane) => lane,
            None => {
                if self.workers.load(Ordering::Acquire) >= MAX_LEASES {
                    return None;
                }
                self.workers.fetch_add(1, Ordering::AcqRel);
                let worker = WorkerPermit(Arc::clone(&self.workers));
                let (sender, receiver) = mpsc::sync_channel::<Job>(MAX_PER_DESTINATION);
                std::thread::Builder::new()
                    .name("download-destination-io".into())
                    .stack_size(STACK_BYTES)
                    .spawn(move || {
                        let _worker = worker;
                        for job in receiver {
                            let _ = catch_unwind(AssertUnwindSafe(job));
                        }
                    })
                    .ok()?;
                let lane = Arc::new(Lane {
                    sender,
                    active: Arc::new(AtomicUsize::new(0)),
                });
                lanes.push((key, Arc::downgrade(&lane)));
                lane
            }
        };
        if lane.active.load(Ordering::Acquire) >= MAX_PER_DESTINATION {
            return None;
        }
        self.active.fetch_add(1, Ordering::AcqRel);
        lane.active.fetch_add(1, Ordering::AcqRel);
        Some(Destination {
            permit: Arc::new(Permit {
                total: Arc::clone(&self.active),
                destination: Arc::clone(&lane.active),
            }),
            lane,
            timeout: self.timeout,
        })
    }
}

struct WorkerPermit(Arc<AtomicUsize>);

impl Drop for WorkerPermit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

fn executor() -> &'static Executor {
    static EXECUTOR: OnceLock<Executor> = OnceLock::new();
    EXECUTOR.get_or_init(|| Executor::new(TIMEOUT))
}

pub(crate) fn acquire(folder: &Path) -> Option<Destination> {
    executor().acquire(folder)
}

pub(crate) fn cleanup_deadline() -> Deadline {
    Deadline(Instant::now() + TIMEOUT)
}

#[cfg(test)]
pub(crate) fn test_deadline() -> Deadline {
    cleanup_deadline()
}

impl Destination {
    /// Dispatch an operation that already owns this same lease (a reservation).
    /// The operation consumes that owner; this handle only schedules its job.
    pub(crate) fn operation(&self) -> Self {
        Self {
            lane: Arc::clone(&self.lane),
            permit: Arc::clone(&self.permit),
            timeout: self.timeout,
        }
    }

    /// Consuming the lease permits only one queued operation per admitted
    /// download. Completion never waits for the folder or accepts late results.
    pub(crate) fn run<T: Send + 'static>(
        self,
        work: impl FnOnce(Self, Deadline) -> io::Result<T> + Send + 'static,
        completed: impl FnOnce(io::Result<T>) + Send + 'static,
        late: impl FnOnce(T) + Send + 'static,
    ) -> bool {
        let deadline = Deadline(Instant::now() + self.timeout);
        let (sender, receiver) = mpsc::sync_channel(0);
        let completion = (Arc::clone(&self.lane), Arc::clone(&self.permit));
        if std::thread::Builder::new()
            .name("download-io-deadline".into())
            .stack_size(STACK_BYTES)
            .spawn(move || {
                let result =
                    receiver.recv_timeout(deadline.0.saturating_duration_since(Instant::now()));
                drop(receiver);
                completed(result.unwrap_or_else(|error| {
                    Err(match error {
                        mpsc::RecvTimeoutError::Timeout => timed_out(),
                        mpsc::RecvTimeoutError::Disconnected => {
                            io::Error::other("download destination I/O failed")
                        }
                    })
                }));
                drop(completion);
            })
            .is_err()
        {
            return false;
        }
        let lane = Arc::clone(&self.lane);
        // Work may consume its lease early. Independently retain admission and
        // the lane through the OS call, result handoff and late disposal.
        let in_flight = (Arc::clone(&self.lane), Arc::clone(&self.permit));
        // Room is reserved by the lease, including for its final cleanup.
        lane.sender
            .try_send(Box::new(move || {
                let result = deadline.check().and_then(|()| work(self, deadline));
                if let Err(undelivered) = sender.send(result)
                    && let Ok(value) = undelivered.0
                {
                    late(value);
                }
                drop(in_flight);
            }))
            .is_ok()
    }

    /// Disposal needs no waiting thread or result buffer. The lease stays in
    /// this job until disposal finishes; saturated destinations admit no more.
    pub(crate) fn cleanup(self, work: impl FnOnce(Deadline) + Send + 'static) {
        let deadline = Deadline(Instant::now() + self.timeout);
        let lane = Arc::clone(&self.lane);
        let queued = lane.sender.try_send(Box::new(move || {
            work(deadline);
            drop(self);
        }));
        // With one queued job per lease the queue cannot be full. A worker
        // catches job panics and stays alive while any lease owns its sender.
        assert!(
            queued.is_ok(),
            "reserved destination cleanup slot unavailable"
        );
    }
}

#[cfg(test)]
pub(crate) fn flush() {
    let deadline = Instant::now() + Duration::from_secs(10);
    while executor().active.load(Ordering::Acquire) != 0 {
        assert!(Instant::now() < deadline, "destination I/O did not drain");
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[cfg(test)]
mod tests;
