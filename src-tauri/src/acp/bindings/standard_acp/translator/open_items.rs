//! ACP streams text, thought and plan updates but never says one ended. The
//! translator closes them from what the protocol does say: answer text after
//! a thought ends that thought block, and the prompt response ends the turn's
//! output. A cancelled or errored prompt leaves them open for the store's turn
//! sweep, which marks unfinished output as cancelled, but still shows each
//! text whole, as a tool call starting does.

use sage_acp_protocol::{
    contract::SafeText,
    intent::{ExternalId, NormalizedPayload},
    lifecycle::ItemLifecycle,
};

use sage_core::limits::LIMITS;

use super::decode::turn_scoped_key;
use super::{AdapterEvent, MAX_EVENTS_PER_FRAME, StandardAcpTranslator, TranslationError};
use crate::acp::redaction::text::ReplyStream;

/// At most this many items close in the prompt response; any beyond it stay
/// open and are swept, so the terminal frame keeps room for diff settlement.
const MAX_CLOSED_AT_TURN_END: usize = MAX_EVENTS_PER_FRAME / 2;

impl StandardAcpTranslator {
    /// A closed block's later thoughts open a new item instead of reopening a completed one.
    pub(super) fn thought_base(&self) -> String {
        match self.thought_block {
            0 => "thought".to_owned(),
            block => format!("thought-{block}"),
        }
    }

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

    pub(super) fn close_thought_block(&mut self) -> Result<Vec<AdapterEvent>, TranslationError> {
        let key = turn_scoped_key(&self.thought_base(), self.turn_scope())?;
        let Some((external_key, payload)) = self.open_items.remove(key.as_str()) else {
            return Ok(Vec::new());
        };
        self.thought_block = self.thought_block.saturating_add(1);
        Ok(vec![self.whole(
            external_key,
            payload,
            ItemLifecycle::Completed,
        )])
    }

    /// Ends the turn's open items: completed when the prompt delivered its
    /// output, and otherwise shown whole but left for the turn's sweep.
    pub(super) fn close_open_items(&mut self, events: &mut Vec<AdapterEvent>, delivered: bool) {
        let open = std::mem::take(&mut self.open_items);
        for (external_key, payload) in open.into_values().take(MAX_CLOSED_AT_TURN_END) {
            if delivered {
                events.push(self.whole(external_key, payload, ItemLifecycle::Completed));
            } else if self.text_snapshots.contains_key(external_key.as_str()) {
                events.push(self.whole(external_key, payload, ItemLifecycle::Streaming));
            }
        }
    }

    /// Each text or thought of the turn still open when the harness's output
    /// ended, shown whole for the turn's sweep to settle.
    pub(crate) fn unfinished_replies(&mut self) -> Vec<AdapterEvent> {
        let mut events = Vec::new();
        if self.pending_prompt.is_some() {
            self.close_open_items(&mut events, false);
        }
        events
    }

    /// Text and thoughts shown whole ahead of a tool call: the harness ended
    /// them to call it, so no word of theirs is still arriving.
    pub(super) fn text_shown_whole_before(&mut self, tool: Vec<AdapterEvent>) -> Vec<AdapterEvent> {
        let open: Vec<_> = self
            .open_items
            .values()
            .filter(|(key, _)| self.text_snapshots.contains_key(key.as_str()))
            .cloned()
            .collect();
        let mut events: Vec<_> = open
            .into_iter()
            .map(|(key, payload)| {
                (
                    self.whole(key, payload.clone(), ItemLifecycle::Streaming),
                    payload,
                )
            })
            .filter_map(|(event, shown)| {
                matches!(&event, AdapterEvent::Item { payload, .. } if *payload != shown)
                    .then_some(event)
            })
            .collect();
        events.extend(tool);
        events
    }

    /// A text or thought shown whole, including the last word that streaming
    /// held back.
    fn whole(
        &mut self,
        external_key: ExternalId,
        payload: NormalizedPayload,
        lifecycle: ItemLifecycle,
    ) -> AdapterEvent {
        let finished = self
            .text_snapshots
            .get_mut(external_key.as_str())
            .map(ReplyStream::finished)
            .and_then(|text| SafeText::new("text", text).ok());
        let payload = match (payload, finished) {
            (NormalizedPayload::Text { .. }, Some(text)) => NormalizedPayload::Text { text },
            (NormalizedPayload::ThoughtSummary { .. }, Some(summary)) => {
                NormalizedPayload::ThoughtSummary { summary }
            }
            (payload, _) => payload,
        };
        AdapterEvent::Item {
            external_key,
            lifecycle,
            tool_state: None,
            payload,
        }
    }
}
