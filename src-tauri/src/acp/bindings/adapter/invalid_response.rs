use crate::acp::transport::Frame;
use serde::{Deserialize, Deserializer, de::IgnoredAny};

#[derive(Deserialize)]
struct ResponseHeader {
    id: u64,
    #[serde(default, deserialize_with = "present")]
    method: bool,
    #[serde(default, deserialize_with = "present")]
    result: bool,
    #[serde(default, deserialize_with = "present")]
    error: bool,
}

fn present<'de, D: Deserializer<'de>>(deserializer: D) -> Result<bool, D::Error> {
    IgnoredAny::deserialize(deserializer).map(|_| true)
}

pub(crate) fn response_id_for_discard(frame: &Frame) -> Option<u64> {
    let header: ResponseHeader = serde_json::from_slice(frame.as_bytes()).ok()?;
    (!header.method && (header.result || header.error)).then_some(header.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(bytes: &[u8]) -> Frame {
        let mut bytes = bytes.to_vec();
        bytes.push(b'\n');
        crate::acp::transport::IncrementalFrameDecoder::new()
            .push_chunk(&bytes)
            .unwrap()
            .unwrap()
    }

    #[test]
    fn discard_header_requires_one_response_id_and_no_method() {
        assert_eq!(
            response_id_for_discard(&frame(br#"{"id":7,"result":null}"#)),
            Some(7)
        );
        assert_eq!(
            response_id_for_discard(&frame(br#"{"id":7,"error":null}"#)),
            Some(7)
        );
        for bytes in [
            br#"{"id":7,"id":8,"result":{}}"#.as_slice(),
            br#"{"id":7,"result":{},"result":{}}"#.as_slice(),
            br#"{"id":7,"method":null,"result":{}}"#.as_slice(),
            br#"{"id":7,"method":"turn/started","params":{}}"#.as_slice(),
            br#"{"id":7}"#.as_slice(),
            br#"{"id":"7","result":{}}"#.as_slice(),
            br#"[{"id":7,"result":{}}]"#.as_slice(),
        ] {
            assert_eq!(response_id_for_discard(&frame(bytes)), None);
        }
    }
}
