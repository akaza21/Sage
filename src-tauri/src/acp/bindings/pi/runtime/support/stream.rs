use crate::acp::bindings::adapter::AdapterRuntimeError;
use crate::acp::bindings::pi::decode::redact_structured_text;
use sage_core::limits::LIMITS;

/// Appends a delta to the text as Pi sent it and returns the whole assembled
/// text redacted: a secret, a path or the home folder may straddle deltas, so
/// no delta is ever redacted alone.
pub(super) fn append_snapshot(
    snapshot: &mut Option<String>,
    delta: &str,
) -> Result<String, AdapterRuntimeError> {
    let mut current = snapshot.take().ok_or(AdapterRuntimeError::Operation)?;
    current
        .len()
        .checked_add(delta.len())
        .filter(|size| within_item_limits(*size))
        .ok_or(AdapterRuntimeError::BatchBytes)?;
    current
        .try_reserve_exact(delta.len())
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    current.push_str(delta);
    let shown = shown_snapshot(&current)?;
    *snapshot = Some(current);
    Ok(shown)
}

/// The redacted form of an assembled text, the only form that leaves the runtime.
pub(super) fn shown_snapshot(raw: &str) -> Result<String, AdapterRuntimeError> {
    let shown = redact_structured_text(raw);
    if !within_item_limits(shown.len()) {
        return Err(AdapterRuntimeError::BatchBytes);
    }
    Ok(shown)
}

fn within_item_limits(size: usize) -> bool {
    size <= LIMITS.wire.normalized_item_bytes as usize
        && size <= LIMITS.wire.item_bytes_per_turn as usize
}
