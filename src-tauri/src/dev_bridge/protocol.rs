use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use tauri::ipc::{InvokeResponse, InvokeResponseBody};
use tokio::sync::{mpsc, watch};

pub(super) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
pub(super) const OUTBOUND_CAPACITY: usize = 8;
pub(super) const MAX_PENDING: usize = 64;
pub(super) const MAX_LISTENERS: usize = 128;

include!(concat!(env!("OUT_DIR"), "/bridge_commands.rs"));

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    pub id: u32,
    pub cmd: String,
    #[serde(default)]
    pub args: Value,
}

#[derive(Serialize)]
struct Success<T> {
    id: u32,
    ok: T,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EventFrame<'a> {
    event: &'a str,
    handler_id: u32,
    payload: &'a RawValue,
}

pub(super) fn admits_command(command: &str) -> bool {
    COMMANDS.contains(&command)
        || matches!(
            command,
            "plugin:event|listen" | "plugin:event|unlisten" | "plugin:event|emit"
        )
}

#[derive(Clone)]
pub(super) struct Outbound {
    sender: mpsc::Sender<String>,
    stop: watch::Sender<bool>,
}

impl Outbound {
    pub fn disconnect(&self) {
        let _ = self.stop.send(true);
    }
    pub fn new(sender: mpsc::Sender<String>, stop: watch::Sender<bool>) -> Self {
        Self { sender, stop }
    }

    pub fn send(&self, frame: impl Serialize) {
        let mut bytes = LimitedBuffer(Vec::new());
        if serde_json::to_writer(&mut bytes, &frame).is_err()
            || self
                .sender
                .try_send(String::from_utf8(bytes.0).expect("JSON UTF-8"))
                .is_err()
        {
            let _ = self.stop.send(true);
        }
    }

    pub fn error(&self, id: u32, message: &str) {
        self.send(json!({ "id": id, "err": message }));
    }

    pub fn respond(&self, id: u32, response: InvokeResponse) {
        match response {
            InvokeResponse::Ok(body) => {
                let length = match &body {
                    InvokeResponseBody::Json(text) => text.len(),
                    InvokeResponseBody::Raw(bytes) => bytes.len(),
                };
                if length > MAX_MESSAGE_BYTES {
                    self.error(id, "response exceeds bridge message limit");
                    return;
                }
                match body {
                    InvokeResponseBody::Json(text) => match RawValue::from_string(text) {
                        Ok(ok) => self.send(Success { id, ok }),
                        Err(_) => self.error(id, "invalid invoke response"),
                    },
                    InvokeResponseBody::Raw(ok) => self.send(Success { id, ok }),
                }
            }
            InvokeResponse::Err(error) => self.send(json!({"id":id,"err":error.0})),
        }
    }

    pub fn event(&self, event: &str, handler_id: u32, payload: &str) {
        if payload.len() > MAX_MESSAGE_BYTES {
            self.disconnect();
            return;
        }
        if let Ok(payload) = serde_json::from_str::<&RawValue>(payload) {
            self.send(EventFrame {
                event,
                handler_id,
                payload,
            });
        }
    }
}

struct LimitedBuffer(Vec<u8>);
impl std::io::Write for LimitedBuffer {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if self.0.len() + bytes.len() > MAX_MESSAGE_BYTES {
            return Err(std::io::Error::other("bridge response limit exceeded"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gate_admits_registered_commands_and_only_the_three_event_commands() {
        for command in [
            "get_bootstrap_snapshot",
            "plugin:event|listen",
            "plugin:event|unlisten",
            "plugin:event|emit",
        ] {
            assert!(admits_command(command), "{command}");
        }
        for command in [
            "missing_command",
            "plugin:dialog|open",
            "plugin:shell|execute",
            "plugin:event|emit_to",
            "plugin:event|listen_extra",
        ] {
            assert!(!admits_command(command), "{command}");
        }
    }

    #[tokio::test]
    async fn outbound_overflow_and_oversized_responses_disconnect_instead_of_growing() {
        let (sender, mut receiver) = mpsc::channel(1);
        let (stop, cancelled) = watch::channel(false);
        let output = Outbound::new(sender, stop);
        output.send(json!({"id":1,"ok":"first"}));
        output.send(json!({"id":2,"ok":"overflow"}));
        assert!(*cancelled.borrow());
        assert_eq!(receiver.recv().await.unwrap(), r#"{"id":1,"ok":"first"}"#);
        assert!(receiver.try_recv().is_err());
        let (sender, mut receiver) = mpsc::channel(1);
        let (stop, cancelled) = watch::channel(false);
        Outbound::new(sender, stop).send(json!({"ok":"a".repeat(MAX_MESSAGE_BYTES)}));
        assert!(*cancelled.borrow());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn invoke_response_envelopes_preserve_values_and_reject_oversize_before_decoding() {
        let (sender, mut receiver) = mpsc::channel(2);
        let (stop, _) = watch::channel(false);
        let output = Outbound::new(sender, stop);
        output.respond(
            7,
            InvokeResponse::Ok(InvokeResponseBody::Json("[1,2,3]".into())),
        );
        assert_eq!(receiver.recv().await.unwrap(), r#"{"id":7,"ok":[1,2,3]}"#);
        output.respond(
            8,
            InvokeResponse::Ok(InvokeResponseBody::Json("a".repeat(MAX_MESSAGE_BYTES + 1))),
        );
        assert_eq!(
            receiver.recv().await.unwrap(),
            r#"{"id":8,"err":"response exceeds bridge message limit"}"#
        );
    }

    #[tokio::test]
    async fn event_frames_preserve_nested_payloads_and_listener_identity() {
        let (sender, mut receiver) = mpsc::channel(1);
        let (stop, _) = watch::channel(false);
        Outbound::new(sender, stop).event(
            "sage:changed",
            17,
            r#"{"text":"hello\n\"world","revision":2}"#,
        );
        let received: Value = serde_json::from_str(&receiver.recv().await.unwrap()).unwrap();
        assert_eq!(
            received,
            json!({"event":"sage:changed","handlerId":17,"payload":{"text":"hello\n\"world","revision":2}})
        );
    }
}
