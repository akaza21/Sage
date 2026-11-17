use std::collections::BTreeMap;

use crate::acp::bindings::adapter::AdapterRuntimeError;
use sage_acp_protocol::{
    contract::SafeText,
    intent::{ExternalId, NormalizedPayload},
    lifecycle::ItemLifecycle,
};
use sage_core::limits::LIMITS;

use super::super::ItemUpdateMode;
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
    values: BTreeMap<String, Snapshot>,
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

    pub(super) fn apply(
        &mut self,
        id: &ExternalId,
        lifecycle: ItemLifecycle,
        mode: ItemUpdateMode,
        payload: NormalizedPayload,
    ) -> Result<NormalizedPayload, AdapterRuntimeError> {
        let Some((kind, incoming)) = parts(&payload) else {
            if lifecycle.is_terminal() {
                self.remove(id.as_str());
            }
            return Ok(payload);
        };
        if lifecycle.is_terminal() {
            self.remove(id.as_str());
            return Ok(payload);
        }
        let limit = LIMITS.wire.normalized_item_bytes as usize;
        let previous = self.values.remove(id.as_str());
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
        self.values.insert(id.as_str().to_owned(), snapshot);
        self.bytes = total;
        Ok(payload)
    }

    /// The item of the same kind as `payload` that a new item `id` ends, shown
    /// whole: OMP streams one reply, thought and plan at a time, so starting
    /// another means the last is done, and at most one of each kind is open
    /// when the turn ends.
    pub(super) fn end_previous(
        &mut self,
        id: &ExternalId,
        payload: &NormalizedPayload,
    ) -> Result<Option<(ExternalId, NormalizedPayload)>, AdapterRuntimeError> {
        let Some((kind, _)) = parts(payload) else {
            return Ok(None);
        };
        if self.values.contains_key(id.as_str()) {
            return Ok(None);
        }
        let Some(previous) = self
            .values
            .iter()
            .find(|(_, snapshot)| snapshot.kind == kind)
            .map(|(previous, _)| previous.clone())
        else {
            return Ok(None);
        };
        let mut snapshot = self
            .values
            .remove(&previous)
            .ok_or(AdapterRuntimeError::Frame)?;
        self.bytes -= snapshot.stream.len();
        let previous =
            ExternalId::new("externalItemId", previous).map_err(|_| AdapterRuntimeError::Frame)?;
        Ok(Some((previous, build(kind, &snapshot.stream.finished())?)))
    }

    /// Each reply still streaming, shown whole: OMP never says a reply ended,
    /// so the turn's end, however it ends, ends them.
    pub(super) fn finish(
        &mut self,
    ) -> Result<Vec<(ExternalId, NormalizedPayload)>, AdapterRuntimeError> {
        let values = std::mem::take(&mut self.values);
        self.bytes = 0;
        values
            .into_iter()
            .map(|(id, mut snapshot)| {
                let id = ExternalId::new("externalItemId", id)
                    .map_err(|_| AdapterRuntimeError::Frame)?;
                Ok((id, build(snapshot.kind, &snapshot.stream.finished())?))
            })
            .collect()
    }

    fn remove(&mut self, id: &str) {
        if let Some(value) = self.values.remove(id) {
            self.bytes -= value.stream.len();
        }
    }
}

fn parts(payload: &NormalizedPayload) -> Option<(TextKind, &str)> {
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
