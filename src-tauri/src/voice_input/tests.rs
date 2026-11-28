use super::{MAX_MESSAGE_CHARS, MAX_TRANSCRIPT_CHARS, Signal, VoiceEvent, VoiceSlot, valid_id};
use serde_json::{Value, json};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn wire(signal: Signal) -> Option<Value> {
    VoiceEvent::from_signal("voice-1", &signal).map(|event| serde_json::to_value(event).unwrap())
}

#[test]
fn engine_signals_become_the_voice_input_payload_the_composer_reads() {
    assert_eq!(
        wire(Signal::Partial("hello".into())),
        Some(json!({"id": "voice-1", "kind": "partial", "text": "hello", "message": null}))
    );
    assert_eq!(
        wire(Signal::Final("hello world".into())),
        Some(json!({"id": "voice-1", "kind": "final", "text": "hello world", "message": null}))
    );
    assert_eq!(
        wire(Signal::Error("No speech detected".into())),
        Some(
            json!({"id": "voice-1", "kind": "error", "text": null, "message": "No speech detected"})
        )
    );
    assert_eq!(wire(Signal::Completed), None);
    assert_eq!(
        serde_json::to_value(VoiceEvent::new("voice-1", "ended", None, None)).unwrap(),
        json!({"id": "voice-1", "kind": "ended", "text": null, "message": null})
    );
}

#[test]
fn transcripts_and_messages_are_capped_by_characters_not_bytes() {
    let spoken = "é".repeat(MAX_TRANSCRIPT_CHARS + 10);
    let event = wire(Signal::Final(spoken)).unwrap();
    let text = event["text"].as_str().unwrap();
    assert_eq!(text.chars().count(), MAX_TRANSCRIPT_CHARS);
    assert_eq!(text.len(), MAX_TRANSCRIPT_CHARS * 2);

    let event = wire(Signal::Error("x".repeat(MAX_MESSAGE_CHARS * 4))).unwrap();
    assert_eq!(event["message"].as_str().unwrap().len(), MAX_MESSAGE_CHARS);
}

#[test]
fn session_identities_are_bounded_ascii_tokens() {
    assert!(valid_id("0f2fd29e-8373-47b3-a0ce-d208732a191d"));
    assert!(valid_id(&"a".repeat(80)));
    assert!(!valid_id(""));
    assert!(!valid_id(&"a".repeat(81)));
    assert!(!valid_id("voice 1"));
    assert!(!valid_id("voice/../1"));
}

struct CloseProbe(Arc<AtomicUsize>);

impl Drop for CloseProbe {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

#[test]
fn autonomous_completion_closes_once_and_ignores_late_completion() {
    let closed = Arc::new(AtomicUsize::new(0));
    let mut slot = VoiceSlot::default();
    assert!(
        slot.replace("current".into(), CloseProbe(closed.clone()))
            .is_none()
    );
    assert!(!slot.complete("stale"));
    assert_eq!(closed.load(Ordering::SeqCst), 0);
    assert!(slot.complete("current"));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert!(!slot.complete("current"));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
}

#[test]
fn stop_and_completion_race_releases_only_the_matching_session() {
    let closed = Arc::new(AtomicUsize::new(0));
    let mut slot = VoiceSlot::default();
    assert!(
        slot.replace("first".into(), CloseProbe(closed.clone()))
            .is_none()
    );
    assert!(slot.take_if("other").is_none());
    drop(slot.take_if("first"));
    assert!(!slot.complete("first"));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
}
