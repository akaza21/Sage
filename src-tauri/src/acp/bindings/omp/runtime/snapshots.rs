use std::collections::BTreeMap;

use crate::acp::{
    bindings::adapter::AdapterRuntimeError,
    contract::SafeText,
    intent::{ExternalId, NormalizedPayload},
    lifecycle::ItemLifecycle,
};
use sage_core::limits::LIMITS;

use super::super::{ItemUpdateMode, wire::normalized_text};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TextKind {
    Text,
    Thought,
    Plan,
}

struct Snapshot {
    kind: TextKind,
    value: String,
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
        let previous = self.values.get(id.as_str());
        let (next, appended_to) = match (mode, previous) {
            (ItemUpdateMode::Append, Some(previous)) if previous.kind == kind => (
                checked_append(&previous.value, incoming)?,
                Some(previous.value.as_str()),
            ),
            (ItemUpdateMode::Append, Some(_)) => return Err(AdapterRuntimeError::Frame),
            (ItemUpdateMode::Append | ItemUpdateMode::Replace, None)
            | (ItemUpdateMode::Replace, Some(_)) => (incoming.to_owned(), None),
        };
        let next = sanitize_snapshot(appended_to, next)?;
        if next.len() > LIMITS.wire.normalized_item_bytes as usize {
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
        let payload = build(kind, &next)?;
        self.values
            .insert(id.as_str().to_owned(), Snapshot { kind, value: next });
        self.bytes = total;
        Ok(payload)
    }

    fn remove(&mut self, id: &str) {
        if let Some(value) = self.values.remove(id) {
            self.bytes -= value.value.len();
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

fn sanitize_snapshot(
    previous: Option<&str>,
    mut joined: String,
) -> Result<String, AdapterRuntimeError> {
    if let Some(previous) = previous
        && previous.ends_with("[redacted]")
    {
        remove_attached_continuation(previous.len(), &mut joined);
    }
    normalized_text::<262144>("snapshot", &joined)
        .map(|value| value.as_str().to_owned())
        .map_err(|_| AdapterRuntimeError::BatchBytes)
}

fn remove_attached_continuation(start: usize, joined: &mut String) {
    let suffix = &joined[start..];
    if suffix.starts_with(char::is_whitespace) {
        return;
    }
    let end = suffix
        .char_indices()
        .find_map(|(index, character)| character.is_whitespace().then_some(index))
        .unwrap_or(suffix.len());
    joined.replace_range(start..start + end, "");
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
