//! Byte length of a value's compact JSON encoding, without building it.

use std::io;

use serde::Serialize;

/// The length `serde_json::to_vec(value)` would have, counted as it is
/// written so no buffer is allocated.
pub(crate) fn encoded_len<T: Serialize + ?Sized>(value: &T) -> serde_json::Result<usize> {
    let mut counter = Counter(0);
    serde_json::to_writer(&mut counter, value)?;
    Ok(counter.0)
}

struct Counter(usize);

impl io::Write for Counter {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.0 += bytes.len();
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::encoded_len;

    #[test]
    fn matches_the_encoded_bytes() {
        let value = serde_json::json!({"text": "naïve \"quoted\"\n", "list": [1, 2.5, null]});
        assert_eq!(
            encoded_len(&value).unwrap(),
            serde_json::to_vec(&value).unwrap().len()
        );
    }
}
