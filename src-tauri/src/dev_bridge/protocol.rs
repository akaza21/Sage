use serde::{Deserialize, Serialize};
use serde_json::{Value, json, value::RawValue};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tauri::ipc::{InvokeResponse, InvokeResponseBody};
use tokio::sync::{mpsc, watch};
use tokio_tungstenite::tungstenite::Message;

pub(super) const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Responses and events wait here for the socket writer; a client that stops
/// reading is disconnected once either bound is reached.
pub(super) const OUTBOUND_FRAMES: usize = 1024;
pub(super) const MAX_QUEUED_BYTES: usize = 16 * 1024 * 1024;
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
    sender: mpsc::Sender<Message>,
    stop: watch::Sender<bool>,
    queued_bytes: Arc<AtomicUsize>,
}

impl Outbound {
    pub fn new(sender: mpsc::Sender<Message>, stop: watch::Sender<bool>) -> Self {
        Self {
            sender,
            stop,
            queued_bytes: Arc::default(),
        }
    }

    pub fn disconnect(&self) {
        let _ = self.stop.send(true);
    }

    /// Called by the socket writer for each frame it takes off the queue.
    pub fn dequeued(&self, frame: &Message) {
        self.queued_bytes.fetch_sub(frame.len(), Ordering::Relaxed);
    }

    fn enqueue(&self, frame: Message) {
        let length = frame.len();
        let queued = self.queued_bytes.fetch_add(length, Ordering::Relaxed) + length;
        if queued > MAX_QUEUED_BYTES || self.sender.try_send(frame).is_err() {
            self.queued_bytes.fetch_sub(length, Ordering::Relaxed);
            self.disconnect();
        }
    }

    pub fn send(&self, frame: impl Serialize) {
        let mut bytes = LimitedBuffer(Vec::new());
        match serde_json::to_writer(&mut bytes, &frame) {
            Ok(()) => self.enqueue(Message::Text(
                String::from_utf8(bytes.0).expect("JSON UTF-8").into(),
            )),
            Err(_) => self.disconnect(),
        }
    }

    pub fn error(&self, id: u32, message: &str) {
        self.send(json!({ "id": id, "err": message }));
    }

    pub fn respond(&self, id: u32, response: InvokeResponse) {
        match response {
            InvokeResponse::Ok(InvokeResponseBody::Json(text)) => {
                if text.len() > MAX_MESSAGE_BYTES {
                    self.error(id, "response exceeds bridge message limit");
                } else {
                    match RawValue::from_string(text) {
                        Ok(ok) => self.send(Success { id, ok }),
                        Err(_) => self.error(id, "invalid invoke response"),
                    }
                }
            }
            // Binary bodies (such as image bytes) travel as a binary frame led by
            // the request id, so the page receives an ArrayBuffer as in the app.
            InvokeResponse::Ok(InvokeResponseBody::Raw(bytes)) => {
                if bytes.len() + 4 > MAX_MESSAGE_BYTES {
                    self.error(id, "response exceeds bridge message limit");
                } else {
                    let mut frame = Vec::with_capacity(bytes.len() + 4);
                    frame.extend_from_slice(&id.to_be_bytes());
                    frame.extend_from_slice(&bytes);
                    self.enqueue(Message::Binary(frame.into()));
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

    fn text(frame: Message) -> String {
        frame.into_text().unwrap().to_string()
    }

    #[tokio::test]
    async fn outbound_overflow_and_oversized_responses_disconnect_instead_of_growing() {
        let (sender, mut receiver) = mpsc::channel(1);
        let (stop, cancelled) = watch::channel(false);
        let output = Outbound::new(sender, stop);
        output.send(json!({"id":1,"ok":"first"}));
        output.send(json!({"id":2,"ok":"overflow"}));
        assert!(*cancelled.borrow());
        assert_eq!(
            text(receiver.recv().await.unwrap()),
            r#"{"id":1,"ok":"first"}"#
        );
        assert!(receiver.try_recv().is_err());
        let (sender, mut receiver) = mpsc::channel(1);
        let (stop, cancelled) = watch::channel(false);
        Outbound::new(sender, stop).send(json!({"ok":"a".repeat(MAX_MESSAGE_BYTES)}));
        assert!(*cancelled.borrow());
        assert!(receiver.try_recv().is_err());
    }

    #[tokio::test]
    async fn a_burst_within_the_byte_budget_is_kept_and_draining_frees_the_budget() {
        let (sender, mut receiver) = mpsc::channel(OUTBOUND_FRAMES);
        let (stop, cancelled) = watch::channel(false);
        let output = Outbound::new(sender, stop);
        for id in 0..200 {
            output.event("sage:changed", id, r#"{"revision":1}"#);
        }
        assert!(!*cancelled.borrow());
        let chunk = "a".repeat(MAX_MESSAGE_BYTES - 64);
        for id in 0..3 {
            output.send(json!({"id":id,"ok":chunk}));
        }
        assert!(!*cancelled.borrow());
        output.send(json!({"id":3,"ok":chunk}));
        assert!(
            *cancelled.borrow(),
            "a fourth large frame exceeds the byte budget"
        );
        while let Ok(frame) = receiver.try_recv() {
            output.dequeued(&frame);
        }
        assert_eq!(output.queued_bytes.load(Ordering::Relaxed), 0);
    }

    #[tokio::test]
    async fn invoke_response_envelopes_preserve_values_and_reject_oversize_before_decoding() {
        let (sender, mut receiver) = mpsc::channel(3);
        let (stop, _) = watch::channel(false);
        let output = Outbound::new(sender, stop);
        output.respond(
            7,
            InvokeResponse::Ok(InvokeResponseBody::Json("[1,2,3]".into())),
        );
        assert_eq!(
            text(receiver.recv().await.unwrap()),
            r#"{"id":7,"ok":[1,2,3]}"#
        );
        output.respond(
            8,
            InvokeResponse::Ok(InvokeResponseBody::Json("a".repeat(MAX_MESSAGE_BYTES + 1))),
        );
        assert_eq!(
            text(receiver.recv().await.unwrap()),
            r#"{"id":8,"err":"response exceeds bridge message limit"}"#
        );
        output.respond(
            9,
            InvokeResponse::Ok(InvokeResponseBody::Raw(vec![0x89, b'P', b'N', b'G'])),
        );
        assert_eq!(
            receiver.recv().await.unwrap(),
            Message::Binary(vec![0, 0, 0, 9, 0x89, b'P', b'N', b'G'].into())
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
        let received: Value = serde_json::from_str(&text(receiver.recv().await.unwrap())).unwrap();
        assert_eq!(
            received,
            json!({"event":"sage:changed","handlerId":17,"payload":{"text":"hello\n\"world","revision":2}})
        );
    }
}
