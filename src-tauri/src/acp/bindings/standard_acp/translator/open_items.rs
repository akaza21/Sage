//! ACP streams text, thought and plan updates but never says one ended. The
//! translator closes them from what the protocol does say: answer text after
//! a thought ends that thought block, and the prompt response ends the turn's
//! output. A cancelled or errored prompt leaves them open for the store's turn
//! sweep, which marks unfinished output as cancelled.

use crate::acp::{intent::NormalizedPayload, lifecycle::ItemLifecycle};
use sage_core::limits::LIMITS;

use super::decode::turn_scoped_key;
use super::{AdapterEvent, MAX_EVENTS_PER_FRAME, StandardAcpTranslator, TranslationError};

/// At most this many items close in the prompt response; any beyond it stay
/// open and are swept, so the terminal frame keeps room for diff settlement.
const MAX_CLOSED_AT_TURN_END: usize = MAX_EVENTS_PER_FRAME / 2;

impl StandardAcpTranslator {
    /// The fallback key base of the current thought block. A closed block's
    /// later thoughts open a new item instead of reopening a completed one.
    pub(super) fn thought_base(&self) -> String {
        match self.thought_block {
            0 => "thought".to_owned(),
            block => format!("thought-{block}"),
        }
    }

    /// A plan item counts toward the turn's item cap like text and tools; a
    /// new plan id past the cap is refused before it is remembered.
    pub(super) fn admit_open_item(&self, key: &str) -> Result<(), TranslationError> {
        if !self.open_items.contains_key(key)
            && self.tracked_item_count() >= LIMITS.wire.items_per_turn as usize
        {
            return Err(TranslationError::ItemLimit);
        }
        Ok(())
    }

    /// Plans are the only open items not already counted as text snapshots.
    pub(super) fn open_plan_count(&self) -> usize {
        self.open_items
            .values()
            .filter(|(_, payload)| matches!(payload, NormalizedPayload::PlanEntries { .. }))
            .count()
    }

    /// Remembers the latest snapshot of an item ACP will not close.
    pub(super) fn remember_open(&mut self, events: &[AdapterEvent]) {
        for event in events {
            if let AdapterEvent::Item {
                external_key,
                lifecycle: ItemLifecycle::Streaming,
                payload:
                    payload @ (NormalizedPayload::Text { .. }
                    | NormalizedPayload::ThoughtSummary { .. }
                    | NormalizedPayload::PlanEntries { .. }),
                ..
            } = event
            {
                self.open_items.insert(
                    external_key.as_str().to_owned(),
                    (external_key.clone(), payload.clone()),
                );
            }
        }
    }

    /// Answer text after an untagged thought ends that thought block.
    pub(super) fn close_thought_block(&mut self) -> Result<Vec<AdapterEvent>, TranslationError> {
        let key = turn_scoped_key(&self.thought_base(), self.turn_scope())?;
        let Some((external_key, payload)) = self.open_items.remove(key.as_str()) else {
            return Ok(Vec::new());
        };
        self.thought_block = self.thought_block.saturating_add(1);
        Ok(vec![completed(external_key, payload)])
    }

    /// Closes the turn's open output before a non-cancelled turn terminal.
    pub(super) fn close_open_items(&mut self, events: &mut Vec<AdapterEvent>) {
        let open = std::mem::take(&mut self.open_items);
        events.extend(
            open.into_values()
                .take(MAX_CLOSED_AT_TURN_END)
                .map(|(external_key, payload)| completed(external_key, payload)),
        );
    }
}

fn completed(
    external_key: crate::acp::intent::ExternalId,
    payload: NormalizedPayload,
) -> AdapterEvent {
    AdapterEvent::Item {
        external_key,
        lifecycle: ItemLifecycle::Completed,
        tool_state: None,
        payload,
    }
}
