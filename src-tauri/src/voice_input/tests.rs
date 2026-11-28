use super::{
    Gate, MAX_MESSAGE_CHARS, MAX_TRANSCRIPT_CHARS, STOPPED_IDS, Signal, VoiceEvent, VoiceSlot,
    coalesced, valid_id,
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
fn a_stopped_id_is_refused_whenever_its_start_runs_and_memory_stays_bounded() {
    let mut slot = VoiceSlot::<CloseProbe>::default();
    // The stop overtakes its own start, then a newer start goes ahead.
    assert!(slot.stop("a").is_none());
    assert!(slot.is_stopped("a"));
    assert!(!slot.is_stopped("b"));
    assert!(slot.stop("a").is_none());
    assert_eq!(slot.stopped.len(), 1, "a repeated stop is kept once");
    for n in 0..20 {
        slot.stop(&format!("x{n}"));
    }
    assert_eq!(slot.stopped.len(), STOPPED_IDS);
    assert!(slot.is_stopped("x19"));
    assert!(!slot.is_stopped("a"), "the oldest stop is evicted");
}

#[test]
fn stopping_a_live_session_releases_it_without_refusing_its_id_later() {
    let closed = Arc::new(AtomicUsize::new(0));
    let mut slot = VoiceSlot::default();
    slot.replace("live".into(), CloseProbe(closed.clone()));
    drop(slot.stop("live"));
    assert_eq!(closed.load(Ordering::SeqCst), 1);
    assert!(slot.stopped.is_empty());
}

#[test]
fn only_one_permission_flow_waits_at_a_time() {
    use std::sync::{Mutex, mpsc};
    let lock = Arc::new(Mutex::new(()));
    let (entered_tx, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel::<()>();
    let first = {
        let lock = lock.clone();
        std::thread::spawn(move || {
            coalesced(&lock, || {
                entered_tx.send(()).unwrap();
                wait.recv().unwrap();
                Ok(())
            })
        })
    };
    entered.recv().unwrap();
    let mut ran = false;
    let second = coalesced(&lock, || {
        ran = true;
        Ok(())
    });
    assert_eq!(
        second.unwrap_err(),
        "Answer the permission prompt that is already open."
    );
    assert!(!ran);
    release.send(()).unwrap();
    assert!(first.join().unwrap().is_ok());
    assert!(coalesced(&lock, || Ok(())).is_ok());
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
