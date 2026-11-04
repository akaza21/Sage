use super::*;

impl CodexOperationGate {
    pub(in crate::acp::service) fn invalidate(&self) {
        let _ = self.invalidate_active().1;
    }

    #[cfg(test)]
    pub(in crate::acp::service) fn invalidate_and_wait(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        let (settlements, cancellation) = self.invalidate_active();
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        cancellation.and(self.wait_until(deadline, settlements))
    }

    /// Project replacement owns this absolute deadline before cancellation is
    /// dispatched. Invalidation and idle settlement both consume that one
    /// budget; no later layer may mint a fresh relative timeout.
    pub(in crate::acp::service) fn invalidate_ephemeral_and_wait_until(
        &self,
        deadline: Instant,
    ) -> Result<(), CodexDiscoveryRunError> {
        if Instant::now() >= deadline {
            return Err(CodexDiscoveryRunError::Timeout);
        }
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        let (settlements, cancellation) = {
            let mut active = self
                .active
                .lock()
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
            Self::request_ephemeral_cancel(&mut active)
        };
        // Cancellation may call back into gate observation, so it must run
        // after releasing `active` and under the same caller-owned deadline.
        if let Some(cancellation) = cancellation {
            cancellation.request_parent_shutdown_until(deadline)?;
        }
        self.wait_until(deadline, settlements)
    }

    pub(in crate::acp::service) fn shutdown(
        &self,
        timeout: Duration,
    ) -> Result<(), CodexDiscoveryRunError> {
        self.shutting_down.store(true, Ordering::Release);
        // Live sessions are parked by runtime shutdown. Waiting on them here
        // spends the whole close budget before park can run.
        let (settlements, cancellation) = self.invalidate_ephemeral();
        let deadline = Instant::now()
            .checked_add(timeout)
            .ok_or(CodexDiscoveryRunError::Timeout)?;
        cancellation.and(self.wait_until(deadline, settlements))
    }

    fn request_ephemeral_cancel(
        active: &mut ActiveState,
    ) -> (Vec<Settlement>, Option<Arc<dyn CodexOperationCancellation>>) {
        match active
            .running
            .iter_mut()
            .find(|slot| slot.class == OperationClass::Ephemeral)
        {
            Some(slot) => {
                slot.cancel_requested = true;
                (vec![slot.settlement.clone()], slot.cancellation.clone())
            }
            None => (Vec::new(), None),
        }
    }

    fn invalidate_ephemeral(&self) -> (Vec<Settlement>, Result<(), CodexDiscoveryRunError>) {
        self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
        let (settlements, cancellation) = match self.active.lock() {
            Ok(mut active) => Self::request_ephemeral_cancel(&mut active),
            Err(_) => return (Vec::new(), Err(CodexDiscoveryRunError::Cleanup)),
        };
        let deadline = Instant::now() + Duration::from_millis(1);
        let result = cancellation.map_or(Ok(()), |value| {
            value.request_parent_shutdown_until(deadline)
        });
        (settlements, result)
    }

    /// Invalidates the admitted binding: every running operation, ephemeral
    /// or live, is asked to stop.
    fn invalidate_active(&self) -> (Vec<Settlement>, Result<(), CodexDiscoveryRunError>) {
        let cancellations: Vec<_> = match self.active.lock() {
            Ok(mut active) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                if active.running.is_empty() {
                    return (Vec::new(), Ok(()));
                }
                active
                    .running
                    .iter_mut()
                    .map(|slot| {
                        slot.cancel_requested = true;
                        (slot.settlement.clone(), slot.cancellation.clone())
                    })
                    .collect()
            }
            Err(_) => {
                self.invalidation_epoch.fetch_add(1, Ordering::AcqRel);
                return (Vec::new(), Err(CodexDiscoveryRunError::Cleanup));
            }
        };
        let deadline = Instant::now() + Duration::from_millis(1);
        let (settlements, cancellations): (Vec<_>, Vec<_>) = cancellations.into_iter().unzip();
        let result = cancellations
            .into_iter()
            .flatten()
            .map(|cancellation| cancellation.request_parent_shutdown_until(deadline))
            .fold(Ok(()), Result::and);
        (settlements, result)
    }

    /// Wait for the exact operations observed at invalidation; later admissions
    /// and other classes cannot replace or consume their settlements.
    fn wait_until(
        &self,
        deadline: Instant,
        settlements: Vec<Settlement>,
    ) -> Result<(), CodexDiscoveryRunError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| CodexDiscoveryRunError::Cleanup)?;
        loop {
            let results = settlements
                .iter()
                .map(|slot| {
                    slot.lock()
                        .map(|value| *value)
                        .map_err(|_| CodexDiscoveryRunError::Cleanup)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if results.iter().all(Option::is_some) {
                return results.into_iter().flatten().fold(Ok(()), Result::and);
            }
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return Err(CodexDiscoveryRunError::Timeout);
            }
            active = self
                .active_changed
                .wait_timeout(active, remaining)
                .map_err(|_| CodexDiscoveryRunError::Cleanup)?
                .0;
        }
    }

    pub(super) fn finish_active(&self, id: u64, settlement: Result<(), CodexDiscoveryRunError>) {
        let mut active = self
            .active
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(slot) = active.slot_mut(id) {
            *slot
                .settlement
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(settlement);
        }
        active.running.retain(|slot| slot.id != id);
        self.active_changed.notify_all();
    }
}
