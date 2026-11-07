use super::{
    AdapterEgress, AdapterEvent, AdapterPumpError, DurableAdapterEvent, LIMITS, NormalizedFact,
};

pub(in crate::acp::runtime::adapter_pump) fn validate_batch(
    events: &[AdapterEvent],
    egress: &[AdapterEgress],
    actual_bytes: usize,
) -> Result<(), AdapterPumpError> {
    if events.len() > LIMITS.queues.journal_facts_per_transaction as usize
        || egress.len() > LIMITS.queues.egress_control_frames_inside_total as usize
    {
        return Err(AdapterPumpError::BatchCapacity);
    }
    let bytes = egress
        .iter()
        .try_fold(0_usize, |sum, value| {
            sum.checked_add(value.frame().wire_len())
        })
        .ok_or(AdapterPumpError::BatchCapacity)?;
    let maximum = (LIMITS.queues.journal_bytes_per_transaction as usize)
        .checked_add(LIMITS.queues.egress_control_bytes_inside_total as usize)
        .ok_or(AdapterPumpError::BatchCapacity)?;
    if bytes > LIMITS.queues.egress_control_bytes_inside_total as usize
        || actual_bytes < bytes
        || actual_bytes > maximum
    {
        return Err(AdapterPumpError::BatchCapacity);
    }
    Ok(())
}

pub(in crate::acp::runtime::adapter_pump) fn validate_deny_pairs(
    durable: &[NormalizedFact],
    egress: &[AdapterEgress],
) -> Result<(), AdapterPumpError> {
    for fact in durable {
        let DurableAdapterEvent::PermissionDeny { intent, .. } = &fact.event else {
            continue;
        };
        let matches = egress
            .iter()
            .filter(|value| {
                value.token().deny_request() == Some((intent.external_request_id.as_str(), false))
            })
            .count();
        if matches != 1 {
            return Err(AdapterPumpError::TeardownRequired);
        }
    }
    Ok(())
}
