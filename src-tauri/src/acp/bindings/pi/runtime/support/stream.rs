use crate::acp::bindings::adapter::AdapterRuntimeError;
use crate::acp::redaction::text::ReplyStream;
use sage_core::limits::LIMITS;

/// Appends a delta to the reply as Pi sent it and returns what may be shown
/// while it streams.
pub(super) fn append_snapshot(
    snapshot: &mut Option<ReplyStream>,
    delta: &str,
) -> Result<String, AdapterRuntimeError> {
    let mut stream = snapshot.take().ok_or(AdapterRuntimeError::Operation)?;
    stream
        .len()
        .checked_add(delta.len())
        .filter(|size| within_item_limits(*size))
        .ok_or(AdapterRuntimeError::BatchBytes)?;
    stream
        .push(delta)
        .map_err(|_| AdapterRuntimeError::BatchBytes)?;
    let shown = bounded(stream.streaming())?;
    *snapshot = Some(stream);
    Ok(shown)
}

/// The whole reply once Pi ends it, however it ended.
pub(super) fn closed_snapshot(
    snapshot: &mut Option<ReplyStream>,
) -> Result<String, AdapterRuntimeError> {
    let mut stream = snapshot.take().ok_or(AdapterRuntimeError::Operation)?;
    bounded(stream.finished())
}

fn bounded(shown: String) -> Result<String, AdapterRuntimeError> {
    if within_item_limits(shown.len()) {
        Ok(shown)
    } else {
        Err(AdapterRuntimeError::BatchBytes)
    }
}

fn within_item_limits(size: usize) -> bool {
    size <= LIMITS.wire.normalized_item_bytes as usize
        && size <= LIMITS.wire.item_bytes_per_turn as usize
}

#[cfg(test)]
mod tests {
    use super::{ReplyStream, closed_snapshot};

    #[test]
    fn a_reply_that_redaction_makes_longer_than_an_item_is_clipped_when_it_closes() {
        let mut snapshot = Some(ReplyStream::new(&"token=abc ".repeat(25_000)));
        let closed = closed_snapshot(&mut snapshot).expect("clipped, not refused");
        assert!(closed.ends_with('\u{2026}'), "{}", closed.len());
    }
}
