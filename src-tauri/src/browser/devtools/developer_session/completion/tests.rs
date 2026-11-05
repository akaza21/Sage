use super::*;

/// The owned payload stands in for a tab slot. Its lifetime is observable
/// without constructing a native WebView; the production State is unchanged.
struct OwnedSession {
    state: Mutex<State>,
    slot: Mutex<Option<Arc<Vec<u8>>>>,
}

impl OwnedSession {
    fn active() -> (Arc<Self>, Weak<Vec<u8>>) {
        let payload = Arc::new(vec![7; 1024]);
        let weak = Arc::downgrade(&payload);
        let mut state = State {
            phase: Phase::Ready("privileged".into()),
            ..State::default()
        };
        state.begin_completion().unwrap();
        (
            Arc::new(Self {
                state: Mutex::new(state),
                slot: Mutex::new(Some(payload)),
            }),
            weak,
        )
    }

    fn detach(&self) {
        let clear = {
            let mut state = self.state.lock().unwrap();
            state.close(true);
            state.detached()
        };
        if clear {
            self.slot.lock().unwrap().take();
        }
    }
}

impl CompletionTarget for OwnedSession {
    fn complete(self: &Arc<Self>) {
        let clear = self.state.lock().unwrap().completed();
        if clear {
            self.slot.lock().unwrap().take();
        }
    }
}

#[test]
fn detach_waits_for_handler_disposal_before_releasing_its_slot() {
    let (session, payload) = OwnedSession::active();
    let completion = Completion::new(&session);
    session.detach();
    assert!(payload.upgrade().is_some());
    assert_eq!(
        session.state.lock().unwrap().begin_completion(),
        Err(BrowserFailure::Unavailable)
    );
    drop(completion);
    assert!(payload.upgrade().is_none());
}

#[test]
fn invoked_completion_still_waits_for_detach_ack_and_acknowledges_only_once() {
    let (session, payload) = OwnedSession::active();
    let mut completion = Completion::new(&session);
    completion.acknowledge();
    assert!(payload.upgrade().is_some());
    session.detach();
    assert!(payload.upgrade().is_none());
    // A late Drop/duplicate invocation cannot discard a replacement's state.
    let replacement = Arc::new(vec![9; 1024]);
    let replacement_weak = Arc::downgrade(&replacement);
    *session.slot.lock().unwrap() = Some(replacement);
    completion.acknowledge();
    drop(completion);
    assert!(replacement_weak.upgrade().is_some());
}

#[test]
fn an_unresolved_handler_does_not_own_the_session_or_its_payload() {
    let (session, payload) = OwnedSession::active();
    let weak_session = Arc::downgrade(&session);
    let completion = Completion::new(&session);
    drop(session);
    assert!(weak_session.upgrade().is_none());
    assert!(payload.upgrade().is_none());
    drop(completion);
}
