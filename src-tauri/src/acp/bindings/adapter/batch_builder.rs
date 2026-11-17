use super::*;

impl AdapterBatchBuilder {
    pub(crate) fn new() -> Self {
        Self {
            events: Vec::new(),
            egress: Vec::new(),
            event_bytes: 0,
            egress_bytes: 0,
        }
    }

    /// Whether `event` fits in what is left of this batch.
    pub(crate) fn fits_event(&self, event: &AdapterEvent) -> Result<bool, AdapterRuntimeError> {
        let bytes = event_bytes(event).ok_or(AdapterRuntimeError::BatchBytes)?;
        Ok(
            self.events.len() < LIMITS.queues.journal_facts_per_transaction as usize
                && self.event_bytes.checked_add(bytes).is_some_and(|next| {
                    next <= LIMITS.queues.journal_bytes_per_transaction as usize
                }),
        )
    }

    /// Whether `egress` fits in what is left of this batch.
    pub(crate) fn fits_egress(&self, egress: &AdapterEgress) -> bool {
        self.egress.len() < LIMITS.queues.egress_control_frames_inside_total as usize
            && self
                .egress_bytes
                .checked_add(egress.frame().wire_len())
                .is_some_and(|next| {
                    next <= LIMITS.queues.egress_control_bytes_inside_total as usize
                })
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.events.is_empty() && self.egress.is_empty()
    }

    pub(crate) fn push_event(&mut self, event: AdapterEvent) -> Result<(), AdapterRuntimeError> {
        if self.events.len() >= LIMITS.queues.journal_facts_per_transaction as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let next = self
            .event_bytes
            .checked_add(event_bytes(&event).ok_or(AdapterRuntimeError::BatchBytes)?)
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if next > LIMITS.queues.journal_bytes_per_transaction as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        self.events
            .try_reserve_exact(1)
            .map_err(|_| AdapterRuntimeError::BatchBytes)?;
        self.events.push(event);
        self.event_bytes = next;
        Ok(())
    }

    pub(crate) fn push_egress(&mut self, egress: AdapterEgress) -> Result<(), AdapterRuntimeError> {
        if self.egress.len() >= LIMITS.queues.egress_control_frames_inside_total as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let next = self
            .egress_bytes
            .checked_add(egress.frame().wire_len())
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if next > LIMITS.queues.egress_control_bytes_inside_total as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        self.egress
            .try_reserve_exact(1)
            .map_err(|_| AdapterRuntimeError::BatchBytes)?;
        self.egress.push(egress);
        self.egress_bytes = next;
        Ok(())
    }

    pub(crate) fn finish(self) -> AdapterBatch {
        AdapterBatch {
            events: self.events,
            egress: self.egress,
            actual_bytes: self.event_bytes + self.egress_bytes,
        }
    }
}
