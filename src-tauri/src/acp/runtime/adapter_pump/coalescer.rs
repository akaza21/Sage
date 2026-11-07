use super::*;

pub(super) const FLUSH_INTERVAL_MILLIS: i64 = 50;
pub(super) const FLUSH_PAYLOAD_BYTES: usize = 4 * 1024;

pub(super) struct NormalizedFact {
    pub event: DurableAdapterEvent,
    pub payload_bytes: usize,
}

impl NormalizedFact {
    pub(super) fn new(event: DurableAdapterEvent, payload_bytes: usize) -> Self {
        Self {
            event,
            payload_bytes,
        }
    }
}

struct PendingFact {
    fact: NormalizedFact,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DeferredIngressState {
    Empty,
    Pending {
        deadline_millis: i64,
        retained_payload_bytes: usize,
    },
    FlushNeeded {
        deadline_millis: i64,
        retained_payload_bytes: usize,
    },
}

pub(super) struct PendingIngress {
    facts: Vec<PendingFact>,
    deadline_millis: Option<i64>,
    retained_payload_bytes: usize,
    actual_bytes: usize,
}

impl PendingIngress {
    pub(super) fn new() -> Self {
        Self {
            facts: Vec::new(),
            deadline_millis: None,
            retained_payload_bytes: 0,
            actual_bytes: 0,
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub(super) fn deadline_millis(&self) -> Option<i64> {
        self.deadline_millis
    }

    pub(super) fn state(&self, now_millis: i64) -> DeferredIngressState {
        let Some(deadline_millis) = self.deadline_millis else {
            return DeferredIngressState::Empty;
        };
        if self.retained_payload_bytes >= FLUSH_PAYLOAD_BYTES || now_millis >= deadline_millis {
            DeferredIngressState::FlushNeeded {
                deadline_millis,
                retained_payload_bytes: self.retained_payload_bytes,
            }
        } else {
            DeferredIngressState::Pending {
                deadline_millis,
                retained_payload_bytes: self.retained_payload_bytes,
            }
        }
    }

    pub(super) fn would_exceed(&self, facts: &[NormalizedFact], source_bytes: usize) -> bool {
        let mut added_count = 0_usize;
        let mut payload = self.retained_payload_bytes;
        for fact in facts {
            let replaced = self.replaceable_index(&fact.event);
            if let Some(index) = replaced {
                payload = payload
                    .saturating_sub(self.facts[index].fact.payload_bytes)
                    .saturating_add(fact.payload_bytes);
            } else {
                added_count = added_count.saturating_add(1);
                payload = payload.saturating_add(fact.payload_bytes);
            }
        }
        let actual = self.actual_bytes.saturating_add(source_bytes);
        self.facts.len().saturating_add(added_count)
            > LIMITS.queues.journal_facts_per_transaction as usize
            || payload > LIMITS.queues.journal_bytes_per_transaction as usize
            || actual > LIMITS.queues.journal_bytes_per_transaction as usize
    }

    pub(super) fn push(
        &mut self,
        facts: Vec<NormalizedFact>,
        source_bytes: usize,
        now_millis: i64,
    ) -> Result<(), AdapterPumpError> {
        if facts.is_empty() {
            return Ok(());
        }
        if self.would_exceed(&facts, source_bytes) {
            return Err(AdapterPumpError::BatchCapacity);
        }
        if self.deadline_millis.is_none() {
            self.deadline_millis = Some(now_millis.saturating_add(FLUSH_INTERVAL_MILLIS));
        }
        self.actual_bytes += source_bytes;
        for fact in facts {
            if let Some(index) = self.replaceable_index(&fact.event) {
                let old = &mut self.facts[index];
                self.retained_payload_bytes = self
                    .retained_payload_bytes
                    .saturating_sub(old.fact.payload_bytes)
                    .saturating_add(fact.payload_bytes);
                old.fact = fact;
                continue;
            }
            self.facts
                .try_reserve_exact(1)
                .map_err(|_| AdapterPumpError::BatchCapacity)?;
            self.retained_payload_bytes += fact.payload_bytes;
            self.facts.push(PendingFact { fact });
        }
        Ok(())
    }

    pub(super) fn take(&mut self) -> Result<(Vec<DurableAdapterEvent>, usize), AdapterPumpError> {
        let actual_bytes = self.actual_bytes;
        let facts = std::mem::take(&mut self.facts);
        self.deadline_millis = None;
        self.retained_payload_bytes = 0;
        self.actual_bytes = 0;
        let mut events = Vec::new();
        events
            .try_reserve_exact(facts.len())
            .map_err(|_| AdapterPumpError::BatchCapacity)?;
        events.extend(facts.into_iter().map(|entry| entry.fact.event));
        Ok((events, actual_bytes))
    }

    pub(super) fn clear(&mut self) {
        self.facts.clear();
        self.deadline_millis = None;
        self.retained_payload_bytes = 0;
        self.actual_bytes = 0;
    }

    fn replaceable_index(&self, event: &DurableAdapterEvent) -> Option<usize> {
        let external_item_id = match event {
            DurableAdapterEvent::Item {
                external_item_id,
                payload,
                ..
            } => {
                if is_pair_anchor(payload) {
                    return None;
                }
                external_item_id
            }
            DurableAdapterEvent::UsageSnapshot { scope, .. } => {
                return self.facts.iter().enumerate().rev().take_while(|(_, pending)| {
                    !matches!(&pending.fact.event, DurableAdapterEvent::Turn { .. })
                }).find_map(|(index, pending)| matches!(&pending.fact.event,
                    DurableAdapterEvent::UsageSnapshot { scope: candidate, .. }
                    if candidate.project_id == scope.project_id && candidate.chat_id == scope.chat_id
                        && candidate.session_id == scope.session_id && candidate.runtime_epoch == scope.runtime_epoch
                        && candidate.turn_id == scope.turn_id
                ).then_some(index));
            }
            // A later title, model or activity state supersedes a pending one,
            // so a burst of progress updates retains one fact per activity.
            DurableAdapterEvent::Session { event, .. } => {
                return self.facts.iter().rposition(|pending| {
                    matches!(&pending.fact.event, DurableAdapterEvent::Session {
                        event: candidate, ..
                    } if candidate.superseded_by(event))
                });
            }
            // Child identity is an immutable fact. A later ordinary Item
            // update with the same external key must not replace it.
            DurableAdapterEvent::ChildThreadItem { .. } => return None,
            _ => return None,
        };
        self.facts
            .iter()
            .enumerate()
            .rev()
            .take_while(|(_, pending)| {
                !matches!(&pending.fact.event, DurableAdapterEvent::Turn { .. })
            })
            .find_map(|(index, pending)| match &pending.fact.event {
                DurableAdapterEvent::Item {
                    external_item_id: candidate,
                    payload,
                    ..
                } if candidate == external_item_id && !is_pair_anchor(payload) => Some(index),
                _ => None,
            })
    }
}

fn is_pair_anchor(payload: &NormalizedPayload) -> bool {
    matches!(payload, NormalizedPayload::FileChangeDiff { .. })
}
