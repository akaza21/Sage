use super::{AdapterPumpError, DurableAdapterEvent, NormalizedFact, SessionScope};

/// Normalizes one admitted session-scoped event. Shared by the Turn pump
/// and the idle paths so every entry point applies the same byte measure.
pub(in crate::acp::runtime::adapter_pump) fn session_fact(
    scope: SessionScope,
    event: sage_acp_protocol::intent::session_event::SessionScopedEvent,
    now_millis: i64,
) -> Result<NormalizedFact, AdapterPumpError> {
    let bytes = serde_json::to_vec(&event)
        .map_err(|_| AdapterPumpError::BatchCapacity)?
        .len();
    Ok(NormalizedFact::new(
        DurableAdapterEvent::Session {
            scope,
            event,
            now_millis,
        },
        bytes,
    ))
}

impl<A, J, R> super::AdapterPump<A, J, R>
where
    A: super::HarnessAdapter,
    J: super::AdapterBatchJournal,
    R: super::ResourceMapper,
{
    /// Queues the session events buffered before this first Turn, so they
    /// commit durably with its first batch. A durable write needs a Turn
    /// owner, so events from an epoch that never starts a Turn stay
    /// display-only and are not kept. The buffer is bounded far below one
    /// journal transaction; a refused push drops the events and is logged.
    pub(crate) fn seed_session_events(
        &mut self,
        mut events: sage_acp_protocol::intent::session_event::PendingSessionEvents,
        now_millis: i64,
    ) {
        let facts = events
            .take()
            .into_iter()
            .filter_map(|event| session_fact(self.session_scope(), event, now_millis).ok())
            .collect::<Vec<_>>();
        let bytes = facts.iter().map(|fact| fact.payload_bytes).sum::<usize>();
        let count = facts.len();
        if count > 0 && self.pending.push(facts, bytes, now_millis).is_err() {
            tracing::warn!(
                count,
                "dropped session events buffered before the first turn"
            );
        }
    }
}
