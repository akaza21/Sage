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
