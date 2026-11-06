//! Disposable folder probes share the download executor's lanes and admission.
//! Expiry releases admission, but retains the lane's probe marker until the OS
//! returns: retries cannot create another blocked call or worker for that folder.

use std::path::PathBuf;
use std::sync::atomic::AtomicBool;

use tokio::sync::{oneshot, watch};

use super::*;

#[derive(Debug)]
pub(super) struct Probe {
    result: watch::Sender<Outcome>,
    permit: Mutex<Option<Arc<Permit>>>,
    deadline: Deadline,
    os_done: AtomicBool,
    timer_done: AtomicBool,
}

#[derive(Clone, Debug)]
enum Outcome {
    Pending,
    Finished(Option<PathBuf>),
}

impl Probe {
    fn finish(&self, result: Option<PathBuf>) {
        let mut permit = self
            .permit
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if permit.take().is_some() {
            // Release admission before waking callers. Late OS results and a
            // reset cannot replace a result that has already been delivered.
            self.result.send_replace(Outcome::Finished(
                result.filter(|_| self.deadline.check().is_ok()),
            ));
        }
    }

    async fn wait(&self) -> Option<PathBuf> {
        let mut result = self.result.subscribe();
        let received = result
            .wait_for(|outcome| matches!(outcome, Outcome::Finished(_)))
            .await
            .ok()?;
        match &*received {
            Outcome::Finished(folder) => folder.clone(),
            Outcome::Pending => None,
        }
    }

    fn retire(self: &Arc<Self>, lane: &Lane) {
        let mut current = lane.probe.lock().unwrap_or_else(|error| error.into_inner());
        if self.os_done.load(Ordering::Acquire)
            && self.timer_done.load(Ordering::Acquire)
            && current
                .as_ref()
                .is_some_and(|probe| Arc::ptr_eq(probe, self))
        {
            current.take();
        }
    }
}

impl Executor {
    pub(crate) async fn probe(
        &self,
        folder: PathBuf,
        work: impl FnOnce(&Path, Deadline) -> Option<PathBuf> + Send + 'static,
    ) -> Option<PathBuf> {
        self.start_probe(folder, work)?.wait().await
    }

    fn start_probe(
        &self,
        folder: PathBuf,
        work: impl FnOnce(&Path, Deadline) -> Option<PathBuf> + Send + 'static,
    ) -> Option<Arc<Probe>> {
        let mut lanes = self.lanes.lock().ok()?;
        let lane = self.lane(&mut lanes, &folder)?;
        let mut current = lane.probe.lock().ok()?;
        if let Some(probe) = current.as_ref() {
            return Some(Arc::clone(probe));
        }
        let permit = self.permit(&lane)?;
        let deadline = Deadline(Instant::now() + self.timeout);
        let (result, _) = watch::channel(Outcome::Pending);
        let probe = Arc::new(Probe {
            result,
            permit: Mutex::new(Some(permit)),
            deadline,
            os_done: AtomicBool::new(false),
            timer_done: AtomicBool::new(false),
        });
        *current = Some(Arc::clone(&probe));
        let pending = Arc::clone(&probe);
        let running = Arc::clone(&lane);
        let (done, result) = oneshot::channel();
        let queued = lane.sender.try_send(Box::new(move || {
            let result = catch_unwind(AssertUnwindSafe(|| {
                deadline.check().ok()?;
                work(&folder, deadline)
            }))
            .ok()
            .flatten();
            // Keep the lane alive even after expiry. Its worker still counts
            // against the shared physical thread limit until this call returns.
            pending.os_done.store(true, Ordering::Release);
            let _ = done.send(result);
            pending.retire(&running);
        }));
        if queued.is_err() {
            current.take();
            probe.finish(None);
        } else {
            let pending = Arc::clone(&probe);
            let running = Arc::clone(&lane);
            tokio::spawn(async move {
                let result = tokio::select! {
                    result = result => result.ok().flatten(),
                    _ = tokio::time::sleep_until(tokio::time::Instant::from_std(deadline.0)) => None,
                    _ = pending.wait() => None,
                };
                // Retain the marker and lane until both halves finish. This
                // bounds timer tasks as well as workers during rapid retries.
                pending.timer_done.store(true, Ordering::Release);
                pending.retire(&running);
                // Remove a completed marker before publishing success, so the
                // next caller checks the filesystem rather than a cached path.
                pending.finish(result);
            });
        }
        Some(probe)
    }

    /// Reset cancels disposable folder checks, without touching download leases
    /// or forgetting workers whose OS calls have not returned.
    pub(crate) fn cancel_probes(&self) {
        let Ok(lanes) = self.lanes.lock() else {
            return;
        };
        for (_, lane) in lanes.iter() {
            if let Some(lane) = lane.upgrade()
                && let Ok(probe) = lane.probe.lock()
                && let Some(probe) = probe.as_ref()
            {
                probe.finish(None);
            }
        }
    }
}

#[cfg(test)]
mod tests;
