use std::time::{Duration, Instant};

use super::*;

impl AcpService {
    pub(crate) fn shutdown_blocking(&self) -> Result<(), crate::acp::runtime::HostError> {
        // An idle release parking a runtime finishes before the lanes close.
        self.idle_release.stop();
        self.file_stages
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clear();
        let started = Instant::now();
        let budget = crate::acp::process::graceful_exit::close_budget();
        self.close_agent_tabs_for_shutdown(started + budget);
        self.clear_lane_channels();
        let settlement = self.read_text_files.terminalize_all();
        if !settlement.wait_until(started + budget) {
            return Err(crate::acp::runtime::HostError::Deadline);
        }
        let mut failure = None;
        #[cfg(all(test, windows))]
        self.record_shutdown_stage("discovery");
        if let Err(error) = self
            .codex_discovery
            .shutdown(budget.saturating_sub(started.elapsed()))
        {
            failure = Some(map_discovery_shutdown(error));
        } else {
            #[cfg(all(test, windows))]
            self.record_shutdown_stage("recovery");
        }
        self.codex_auth.invalidate();
        // Recovery still owns native/durable authority on error. The early
        // return keeps the slot, publisher, and journal alive for exact retry.
        self.shutdown_codex_recovery(started + budget)?;
        let mut runtime_failure = None;
        let runtimes: Vec<_> = self
            .lanes()
            .filter_map(|lane| lane.runtime.begin_shutdown().map(|runtime| (lane, runtime)))
            .collect();
        #[cfg(all(test, windows))]
        if failure.is_none() && !runtimes.is_empty() {
            self.record_shutdown_stage("runtime");
        }
        // Every harness gets its exit grace at the same time.
        let outcomes = crate::acp::process::graceful_exit::close_all(&runtimes, |(_, runtime)| {
            runtime.shutdown(started + budget)
        });
        // Every failed lane keeps its recovery authority before any error returns.
        let mut slot_failure = None;
        for ((lane, runtime), outcome) in runtimes.iter().zip(outcomes) {
            #[cfg(not(all(test, windows)))]
            let _ = runtime;
            if let Err(error) = outcome {
                #[cfg(all(test, windows))]
                self.record_shutdown_stage(runtime.last_shutdown_phase());
                if lane.runtime.shutdown_failed().is_err() {
                    slot_failure = Some(crate::acp::runtime::HostError::RecoveryRequired);
                }
                runtime_failure.get_or_insert(error);
            }
        }
        if let Some(error) = slot_failure.or(runtime_failure) {
            return Err(error);
        }
        let remaining = (started + budget).saturating_duration_since(Instant::now());
        if let Err(error) = self.stop_notifier(remaining) {
            #[cfg(all(test, windows))]
            self.record_shutdown_stage("notifier");
            failure.get_or_insert(error);
        }
        self.journal.close_when_drained();
        if let Err(error) =
            self.wait_journal((started + budget).saturating_duration_since(Instant::now()))
        {
            self.journal.shutdown();
            #[cfg(all(test, windows))]
            self.record_shutdown_stage("journal");
            failure.get_or_insert(error);
        }
        // Every lane finishes; one failure must not skip the others.
        let unfinished = self
            .lanes()
            .filter(|lane| lane.runtime.finish_shutdown().is_err())
            .count();
        if unfinished != 0 {
            #[cfg(all(test, windows))]
            if failure.is_none() {
                self.record_shutdown_stage("finish");
            }
            failure.get_or_insert(crate::acp::runtime::HostError::RecoveryRequired);
        }
        #[cfg(all(test, windows))]
        if failure.is_none() {
            self.record_shutdown_stage("ok");
        }
        failure.map_or(Ok(()), Err)
    }

    fn clear_lane_channels(&self) {
        for lane in self.lanes() {
            lane.runtime.config.clear_all();
            lane.runtime.goal.clear_all();
        }
    }

    fn stop_notifier(&self, timeout: Duration) -> Result<(), crate::acp::runtime::HostError> {
        let Some(notifier) = self.notifier.as_ref().map(Arc::clone) else {
            return Ok(());
        };
        let clean = Arc::new(AtomicBool::new(false));
        let completed = Arc::clone(&clean);
        let mut thread = BoundedThread::spawn("sage-acp-notifier-stop", move || {
            let stopped = tauri::async_runtime::block_on(notifier.shutdown_with_timeout(timeout));
            completed.store(stopped, Ordering::Release);
        })
        .map_err(|_| crate::acp::runtime::HostError::Unavailable)?;
        thread.wait(timeout).map_err(map_thread_wait)?;
        if clean.load(Ordering::Acquire) {
            Ok(())
        } else {
            Err(crate::acp::runtime::HostError::Deadline)
        }
    }

    fn wait_journal(&self, timeout: Duration) -> Result<(), crate::acp::runtime::HostError> {
        let mut thread = self
            .journal_thread
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(worker) = thread.as_mut() else {
            return Ok(());
        };
        match worker.wait(timeout) {
            Ok(()) => {
                *thread = None;
                Ok(())
            }
            Err(error) => Err(map_thread_wait(error)),
        }
    }
}

impl Drop for AcpService {
    fn drop(&mut self) {
        self.browser_attachments.retire_all();
        self.clear_lane_channels();
        let started = Instant::now();
        let settlement = self.read_text_files.terminalize_all();
        let budget = crate::acp::process::graceful_exit::close_budget();
        let recovery_deadline = started + budget;
        // A timed-out writer retains Arc-owned state and may still emit. Drop
        // must not claim a completed ACP teardown in that effect-unknown case.
        if !settlement.wait_until(recovery_deadline) {
            return;
        }
        let mut recovery_attempts = 0_u8;
        while recovery_attempts < 2
            && Instant::now() < recovery_deadline
            && self.has_retained_recovery()
            && self.shutdown_codex_recovery(recovery_deadline).is_err()
        {
            recovery_attempts += 1;
        }
        #[cfg(test)]
        DROP_RECOVERY_ATTEMPTS.fetch_add(usize::from(recovery_attempts), Ordering::AcqRel);
        let _ = self
            .codex_discovery
            .shutdown(budget.saturating_sub(started.elapsed()));
        self.codex_auth.invalidate();
        let mut runtime_stopped = vec![true; self.lanes.len()];
        let runtimes: Vec<_> = self
            .lanes()
            .enumerate()
            .filter_map(|(index, lane)| {
                lane.runtime
                    .begin_shutdown()
                    .map(|runtime| (index, lane, runtime))
            })
            .collect();
        let outcomes =
            crate::acp::process::graceful_exit::close_all(&runtimes, |(_, _, runtime)| {
                runtime.shutdown(started + budget)
            });
        for ((index, lane, _), outcome) in runtimes.iter().zip(outcomes) {
            if outcome.is_err() {
                let _ = lane.runtime.shutdown_failed();
                runtime_stopped[*index] = false;
            }
        }
        let _ = self.stop_notifier(budget.saturating_sub(started.elapsed()));
        self.journal.close_when_drained();
        if self
            .wait_journal(budget.saturating_sub(started.elapsed()))
            .is_err()
        {
            self.journal.shutdown();
        }
        for (index, lane) in self.lanes().enumerate() {
            if runtime_stopped[index] {
                let _ = lane.runtime.finish_shutdown();
            }
        }
    }
}

#[cfg(test)]
pub(super) static DROP_RECOVERY_ATTEMPTS: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

fn map_thread_wait(error: ThreadWaitError) -> crate::acp::runtime::HostError {
    match error {
        ThreadWaitError::Timeout => crate::acp::runtime::HostError::Deadline,
        ThreadWaitError::Panicked => crate::acp::runtime::HostError::Unavailable,
    }
}

pub(super) fn map_discovery_shutdown(
    error: crate::acp::bindings::codex::CodexDiscoveryRunError,
) -> crate::acp::runtime::HostError {
    match error {
        crate::acp::bindings::codex::CodexDiscoveryRunError::Timeout => {
            crate::acp::runtime::HostError::Deadline
        }
        crate::acp::bindings::codex::CodexDiscoveryRunError::Cleanup => {
            crate::acp::runtime::HostError::RecoveryRequired
        }
        _ => crate::acp::runtime::HostError::Unavailable,
    }
}
