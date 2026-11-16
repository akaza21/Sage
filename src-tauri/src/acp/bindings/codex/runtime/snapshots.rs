use std::collections::BTreeMap;

use crate::acp::bindings::adapter::AdapterRuntimeError;
use sage_acp_protocol::{contract::SafeText, intent::NormalizedPayload, lifecycle::ItemLifecycle};
use sage_core::limits::LIMITS;

use super::super::{
    incoming::{ItemUpdateMode, sanitize},
    state::ScopedExternalId,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextKind {
    Text,
    Thought,
    Plan,
}

/// The text as the harness sent it, assembled from its deltas; only its
/// redacted form ever leaves the runtime.
struct Snapshot {
    kind: TextKind,
    value: String,
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
        let previous = self.values.get(&key);
        let next = match (mode, previous) {
            (ItemUpdateMode::Append, Some(previous)) if previous.kind == kind => {
                checked_append(&previous.value, incoming)?
            }
            (ItemUpdateMode::Append, Some(_)) => return Err(AdapterRuntimeError::Frame),
            (ItemUpdateMode::Append | ItemUpdateMode::Replace, None)
            | (ItemUpdateMode::Replace, Some(_)) => incoming.to_owned(),
        };
        // Redact the whole assembled text, never each delta alone: a secret,
        // a path or the home folder may straddle delta boundaries.
        let shown = sanitize(&next);
        if next.len() > LIMITS.wire.normalized_item_bytes as usize
            || shown.len() > LIMITS.wire.normalized_item_bytes as usize
        {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        let previous_bytes = previous.map_or(0, |value| value.value.len());
        let total = self
            .bytes
            .checked_sub(previous_bytes)
            .and_then(|value| value.checked_add(next.len()))
            .ok_or(AdapterRuntimeError::BatchBytes)?;
        if total > LIMITS.wire.item_bytes_per_turn as usize {
            return Err(AdapterRuntimeError::BatchBytes);
        }
        if previous.is_none() && self.values.len() >= LIMITS.wire.items_per_turn as usize {
            return Err(AdapterRuntimeError::BatchCount);
        }
        let payload = build(kind, &shown)?;
        self.values.insert(key, Snapshot { kind, value: next });
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
            self.bytes -= value.value.len();
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

fn checked_append(previous: &str, delta: &str) -> Result<String, AdapterRuntimeError> {
    let length = previous
        .len()
        .checked_add(delta.len())
        .ok_or(AdapterRuntimeError::BatchBytes)?;
    if length > LIMITS.wire.normalized_item_bytes as usize {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    let mut output = String::new();
    output
        .try_reserve_exact(length)
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    output.push_str(previous);
    output.push_str(delta);
    Ok(output)
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
