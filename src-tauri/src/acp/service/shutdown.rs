use super::*;

impl AcpService {
    pub(crate) fn shutdown_blocking(&self) -> Result<(), crate::acp::runtime::HostError> {
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        let mut failure = None;
        #[cfg(test)]
        self.record_shutdown_stage("discovery");
        if let Err(error) = self
            .codex_discovery
            .shutdown(budget.saturating_sub(started.elapsed()))
        {
            failure = Some(map_discovery_shutdown(error));
        } else {
            #[cfg(test)]
            self.record_shutdown_stage("recovery");
        }
        self.codex_auth.invalidate();
        // Recovery still owns native/durable authority on error. The early
        // return keeps the slot, publisher, and journal alive for exact retry.
        self.shutdown_codex_recovery(started + budget)?;
        if let Some(runtime) = self.runtime.begin_shutdown() {
            #[cfg(test)]
            if failure.is_none() {
                self.record_shutdown_stage("runtime");
            }
            if let Err(error) = runtime.shutdown(started + budget) {
                #[cfg(test)]
                self.record_shutdown_stage(runtime.last_shutdown_phase());
                self.runtime
                    .shutdown_failed()
                    .map_err(|_| crate::acp::runtime::HostError::RecoveryRequired)?;
                return Err(error);
            }
        }
        let remaining = (started + budget).saturating_duration_since(Instant::now());
        if let Err(error) = self.stop_notifier(remaining) {
            #[cfg(test)]
            self.record_shutdown_stage("notifier");
            failure.get_or_insert(error);
        }
        self.journal.close_when_drained();
        if let Err(error) =
            self.wait_journal((started + budget).saturating_duration_since(Instant::now()))
        {
            self.journal.shutdown();
            #[cfg(test)]
            self.record_shutdown_stage("journal");
            failure.get_or_insert(error);
        }
        if self.runtime.finish_shutdown().is_err() {
            #[cfg(test)]
            if failure.is_none() {
                self.record_shutdown_stage("finish");
            }
            failure.get_or_insert(crate::acp::runtime::HostError::RecoveryRequired);
        }
        #[cfg(test)]
        if failure.is_none() {
            self.record_shutdown_stage("ok");
        }
        failure.map_or(Ok(()), Err)
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
        let started = Instant::now();
        let budget = Duration::from_millis(crate::acp::limits::LIMITS.deadlines_ms.explicit_close);
        let recovery_deadline = started + budget;
        let mut recovery_attempts = 0_u8;
        while recovery_attempts < 2
            && Instant::now() < recovery_deadline
            && self
                .codex_recovery
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
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
        let mut runtime_stopped = true;
        if let Some(runtime) = self.runtime.begin_shutdown()
            && runtime.shutdown(started + budget).is_err()
        {
            let _ = self.runtime.shutdown_failed();
            runtime_stopped = false;
        }
        let _ = self.stop_notifier(budget.saturating_sub(started.elapsed()));
        self.journal.close_when_drained();
        if self
            .wait_journal(budget.saturating_sub(started.elapsed()))
            .is_err()
        {
            self.journal.shutdown();
        }
        if runtime_stopped {
            let _ = self.runtime.finish_shutdown();
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
