use std::collections::BTreeMap;

use crate::acp::bindings::adapter::AdapterRuntimeError;
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload, lifecycle::ItemLifecycle};
use sage_core::limits::LIMITS;

use super::super::{incoming::ItemUpdateMode, state::ScopedExternalId};
use crate::acp::redaction::text::ReplyStream;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextKind {
    Text,
    Thought,
    Plan,
}

/// The text as the harness sent it; only its redacted form leaves the runtime.
struct Snapshot {
    kind: TextKind,
    stream: ReplyStream,
}

pub(super) struct ItemSnapshots {
    values: BTreeMap<(String, String), Snapshot>,
    bytes: usize,
}

impl ItemSnapshots {
    pub(super) fn new() -> Self {
        Self {
            values: BTreeMap::new(),
            bytes: 0,
        }
    }

    pub(super) fn clear(&mut self) {
        self.values.clear();
        self.bytes = 0;
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.values.len()
    }

    #[cfg(test)]
    pub(super) fn bytes(&self) -> usize {
        self.bytes
    }

    pub(super) fn apply(
        &mut self,
        id: &ScopedExternalId,
        lifecycle: ItemLifecycle,
        mode: ItemUpdateMode,
        payload: NormalizedPayload,
    ) -> Result<NormalizedPayload, AdapterRuntimeError> {
        let Some((kind, incoming)) = text_parts(&payload) else {
            if lifecycle.is_terminal() {
                self.remove(id);
            }
            return Ok(payload);
        };
        if lifecycle.is_terminal() {
            self.remove(id);
            return Ok(payload);
        }
        let key = key(id)?;
        let limit = LIMITS.wire.normalized_item_bytes as usize;
        let previous = self.values.remove(&key);
        let new_item = previous.is_none();
        let previous_bytes = previous.as_ref().map_or(0, |value| value.stream.len());
        // Deltas are assembled as sent and the whole reply is redacted; a
        // replacing payload is a complete text and is shown whole.
        let (mut snapshot, streamed) = match (mode, previous) {
            (ItemUpdateMode::Append, Some(mut previous)) if previous.kind == kind => {
                if previous_bytes.saturating_add(incoming.len()) > limit {
                    return Err(AdapterRuntimeError::BatchBytes);
                }
                previous
                    .stream
                    .push(incoming)
                    .map_err(|_| AdapterRuntimeError::BatchBytes)?;
                (previous, true)
            }
            (ItemUpdateMode::Append, Some(_)) => return Err(AdapterRuntimeError::Frame),
            (mode, _) => (
                Snapshot {
                    kind,
                    stream: ReplyStream::new(incoming),
                },
                mode == ItemUpdateMode::Append,
            ),
        };
        let raw_bytes = snapshot.stream.len();
        let shown = if streamed {
            snapshot.stream.streaming()
        } else {
            snapshot.stream.finished()
        };
        if raw_bytes > limit || shown.len() > limit {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        let total = self
            .bytes
            .checked_sub(previous_bytes)
            .and_then(|value| value.checked_add(raw_bytes))
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if total > LIMITS.wire.item_bytes_per_turn as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        if new_item && self.values.len() >= LIMITS.wire.items_per_turn as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let payload = build(kind, &shown)?;
        self.values.insert(key, snapshot);
        self.bytes = total;
        Ok(payload)
    }

    fn remove(&mut self, id: &ScopedExternalId) {
        let Some(turn) = id.turn_id.as_ref() else {
            return;
        };
        if let Some(value) = self
            .values
            .remove(&(turn.as_str().to_owned(), id.external_id.as_str().to_owned()))
        {
            self.bytes -= value.stream.len();
        }
    }
}

fn key(id: &ScopedExternalId) -> Result<(String, String), AdapterRuntimeError> {
    let turn = id.turn_id.as_ref().ok_or(AdapterRuntimeError::Operation)?;
    Ok((turn.as_str().to_owned(), id.external_id.as_str().to_owned()))
}

fn text_parts(payload: &NormalizedPayload) -> Option<(TextKind, &str)> {
    match payload {
        NormalizedPayload::Text { text } => Some((TextKind::Text, text.as_str())),
        NormalizedPayload::ThoughtSummary { summary } => {
            Some((TextKind::Thought, summary.as_str()))
        }
        NormalizedPayload::Plan { summary } => Some((TextKind::Plan, summary.as_str())),
        _ => None,
    }
}

fn build(kind: TextKind, value: &str) -> Result<NormalizedPayload, AdapterRuntimeError> {
    let value =
        SafeText::new("snapshot", value.to_owned()).map_err(|_| AdapterRuntimeError::BatchBytes)?;
    Ok(match kind {
        TextKind::Text => NormalizedPayload::Text { text: value },
        TextKind::Thought => NormalizedPayload::ThoughtSummary { summary: value },
        TextKind::Plan => NormalizedPayload::Plan { summary: value },
    })
}
