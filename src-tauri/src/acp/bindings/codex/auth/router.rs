use std::collections::VecDeque;

use serde_json::Value;

use crate::acp::transport::Frame;

use super::{CodexAuthError, CodexAuthNotification, CodexAuthProtocol};

const ROUTER_FRAMES: usize = 4;
const ROUTER_BYTES: usize = 20 * 1024;

pub(super) enum RoutedFrame {
    Response { id: u64, frame: Frame },
    Notification,
}

pub(super) struct AuthFrameRouter {
    notifications: VecDeque<(usize, CodexAuthNotification)>,
    retained_bytes: usize,
}

impl AuthFrameRouter {
    pub(super) fn new() -> Self {
        Self {
            notifications: VecDeque::new(),
            retained_bytes: 0,
        }
    }

    pub(super) fn route(
        &mut self,
        protocol: &CodexAuthProtocol,
        frame: Frame,
    ) -> Result<RoutedFrame, CodexAuthError> {
        let value: Value =
            serde_json::from_slice(frame.as_bytes()).map_err(|_| CodexAuthError::Protocol)?;
        let object = value.as_object().ok_or(CodexAuthError::Protocol)?;
        if let Some(id) = object.get("id").and_then(Value::as_u64) {
            return Ok(RoutedFrame::Response { id, frame });
        }
        let notification = protocol.accept_notification(&frame)?;
        let bytes = frame.len();
        let retained = self
            .retained_bytes
            .checked_add(bytes)
            .filter(|retained| *retained <= ROUTER_BYTES)
            .ok_or(CodexAuthError::Protocol)?;
        if self.notifications.len() >= ROUTER_FRAMES {
            return Err(CodexAuthError::Protocol);
        }
        self.notifications.push_back((bytes, notification));
        self.retained_bytes = retained;
        Ok(RoutedFrame::Notification)
    }

    pub(super) fn pop_notification(&mut self) -> Option<CodexAuthNotification> {
        let (bytes, notification) = self.notifications.pop_front()?;
        self.retained_bytes = self.retained_bytes.saturating_sub(bytes);
        Some(notification)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::acp::{
        bindings::codex::parse_packet_metadata, contract::RuntimeEpoch,
        transport::IncrementalFrameDecoder,
    };
    use serde_json::json;

    #[test]
    fn count_and_byte_overflow_are_explicit() {
        let protocol = protocol();
        let mut count = AuthFrameRouter::new();
        for _ in 0..ROUTER_FRAMES {
            count.route(&protocol, completed("ok")).unwrap();
        }
        assert!(count.route(&protocol, completed("overflow")).is_err());

        let mut bytes = AuthFrameRouter::new();
        let large = "x".repeat(6_000);
        for _ in 0..3 {
            bytes.route(&protocol, completed(&large)).unwrap();
        }
        assert!(bytes.route(&protocol, completed(&large)).is_err());
    }

    fn completed(error: &str) -> Frame {
        frame(json!({"method":"account/login/completed","params":{
            "loginId":"7be3f527-7a5a-41e5-9e5f-a1a74b42f86b",
            "success":false,"error":error
        }}))
    }

    fn protocol() -> CodexAuthProtocol {
        let packet = parse_packet_metadata(include_bytes!(
            "../../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .unwrap();
        CodexAuthProtocol::new(&packet, [9; 32], RuntimeEpoch::new(81).unwrap())
    }

    fn frame(value: Value) -> Frame {
        let mut bytes = serde_json::to_vec(&value).unwrap();
        bytes.push(b'\n');
        let mut decoder = IncrementalFrameDecoder::new();
        let mut result = None;
        for chunk in bytes.chunks(8 * 1024) {
            if let Some(frame) = decoder.push_chunk(chunk).unwrap() {
                result = Some(frame);
            }
        }
        result.unwrap()
    }
}
