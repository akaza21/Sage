use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch, or 0 when the clock reads earlier.
pub fn now_millis() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}
