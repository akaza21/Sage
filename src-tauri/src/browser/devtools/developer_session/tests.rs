use super::*;

#[test]
fn a_late_attach_after_revocation_is_detached_instead_of_published() {
    let mut state = State::default();
    assert_eq!(state.close(true), None);
    assert!(matches!(state.phase, Phase::Closing));
    assert_eq!(
        state.attached("late-session".into()),
        Some("late-session".into())
    );
    assert_eq!(state.close(true), None, "detach is issued only once");
}

#[test]
fn hidden_owned_work_finishes_before_sleep_but_revocation_ends_it_immediately() {
    let mut state = State {
        phase: Phase::Ready("privileged".into()),
        busy: true,
        ..State::default()
    };
    assert_eq!(state.close(false), None);
    assert!(matches!(state.phase, Phase::Ready(_)));
    assert!(state.sleep);
    state.busy = false;
    assert_eq!(state.close(true), Some("privileged".into()));

    let mut active = State {
        phase: Phase::Ready("active".into()),
        busy: true,
        ..State::default()
    };
    assert_eq!(active.close(true), Some("active".into()));
    assert!(matches!(active.phase, Phase::Closing));
}
