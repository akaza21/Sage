use crate::acp::bindings::pi::decode::redact_with_tail;
use crate::acp::{bindings::adapter::AdapterRuntimeError, limits::LIMITS};

pub(super) fn append_snapshot(
    snapshot: &mut Option<String>,
    redacted_tail: &mut bool,
    delta: &str,
    delta_redacted_tail: bool,
) -> Result<String, AdapterRuntimeError> {
    let current = snapshot.take().ok_or(AdapterRuntimeError::Operation)?;
    let length = current
        .len()
        .checked_add(delta.len())
        .filter(|size| {
            *size <= LIMITS.wire.normalized_item_bytes as usize
                && *size <= LIMITS.wire.item_bytes_per_turn as usize
        })
        .ok_or(AdapterRuntimeError::BatchBytes)?;
    let mut next = String::new();
    next.try_reserve_exact(length)
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    next.push_str(&current);
    next.push_str(delta);
    let continuing_redaction = *redacted_tail && !delta.contains(char::is_whitespace);
    if *redacted_tail && !delta.starts_with(char::is_whitespace) {
        let end = delta
            .char_indices()
            .find_map(|(index, character)| character.is_whitespace().then_some(index))
            .unwrap_or(delta.len());
        next.replace_range(current.len()..current.len() + end, "");
    }
    let (next, joined_redacted_tail) = redact_with_tail(&next);
    if next.len() > LIMITS.wire.normalized_item_bytes as usize
        || next.len() > LIMITS.wire.item_bytes_per_turn as usize
    {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    *snapshot = Some(next.clone());
    *redacted_tail = continuing_redaction || delta_redacted_tail || joined_redacted_tail;
    Ok(next)
}
