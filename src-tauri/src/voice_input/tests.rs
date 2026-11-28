use super::{
    Gate, MAX_MESSAGE_CHARS, MAX_TRANSCRIPT_CHARS, Signal, VoiceEvent, VoiceSlot, valid_id,
};
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

#[test]
fn only_the_newest_unstopped_start_may_open_the_microphone() {
    let mut slot = VoiceSlot::<CloseProbe>::default();
    slot.begin("a");
    slot.begin("b");
    assert!(!slot.is_pending("a"), "a newer start supersedes a");
    assert!(slot.stop("a").is_none());
    assert!(slot.stop("b").is_none());
    assert!(!slot.claim("a"));
    assert!(!slot.claim("b"));
    assert!(slot.pending.is_none(), "stops never record ids");

    slot.begin("c");
    assert!(slot.is_pending("c"));
    assert!(slot.claim("c"));
    assert!(!slot.claim("c"), "a start is admitted once");
}

#[test]
fn stopping_a_live_session_releases_it_and_leaves_a_pending_start_alone() {
    let closed = Arc::new(AtomicUsize::new(0));
    let mut slot = VoiceSlot::default();
    slot.replace("live".into(), CloseProbe(closed.clone()));
    slot.begin("next");
    drop(slot.stop("live"));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert!(slot.claim("next"));
}

#[test]
fn gate_close_waits_for_an_in_flight_delivery_and_blocks_later_ones() {
    use std::sync::{Barrier, mpsc};
    let gate = Arc::new(Gate::default());
    let entered = Arc::new(Barrier::new(2));
    let (release, wait) = mpsc::channel::<()>();
    let delivering = {
        let (gate, entered) = (gate.clone(), entered.clone());
        std::thread::spawn(move || {
            gate.deliver(|| {
                entered.wait();
                wait.recv().unwrap();
                false
            })
        })
    };
    entered.wait();
    let (closed_tx, closed_rx) = mpsc::channel();
    let closer = {
        let gate = gate.clone();
        std::thread::spawn(move || closed_tx.send(gate.close()).unwrap())
    };
    assert!(
        closed_rx
            .recv_timeout(std::time::Duration::from_millis(100))
            .is_err(),
        "close must wait for the delivery in flight"
    );
    release.send(()).unwrap();
    delivering.join().unwrap();
    closer.join().unwrap();
    assert!(closed_rx.recv().unwrap());
    let mut ran = false;
    gate.deliver(|| {
        ran = true;
        false
    });
    assert!(!ran);
}

#[test]
fn a_finished_delivery_closes_the_gate_so_shutdown_does_not_complete_twice() {
    let gate = Gate::default();
    let mut runs = 0;
    gate.deliver(|| {
        runs += 1;
        true
    });
    gate.deliver(|| {
        runs += 1;
        false
    });
    assert_eq!(runs, 1);
    assert!(!gate.close());
}
