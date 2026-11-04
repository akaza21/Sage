use super::tests::step;
use super::*;

#[test]
fn a_hidden_tab_can_finish_suspending_across_window_minimize_and_restore() {
    let now = Instant::now();
    let hidden = step(
        Policy::default(),
        false,
        Pins::default(),
        Event::Changed,
        now,
    )
    .unwrap();
    let attempt = step(
        hidden.next,
        false,
        Pins::default(),
        Event::Timer(hidden.next.generation),
        now + SUSPEND_AFTER,
    )
    .unwrap();
    let mut current = attempt.next;
    for minimized in [true, false] {
        if let Some(change) = next_state(
            Inputs {
                current,
                placed: false,
                minimized,
                pins: Pins::default(),
                event: Event::Changed,
            },
            now + SUSPEND_AFTER,
        ) {
            current = change.next;
        }
    }
    let completed = step(
        current,
        false,
        Pins::default(),
        Event::Completed(attempt.next.generation, true),
        now + SUSPEND_AFTER,
    )
    .expect("window-only signals must not lose a successful suspension");
    assert_eq!(completed.next.state, MemoryState::Suspended);
    assert!(!completed.resume);
}

#[test]
fn activity_cancels_a_pending_attempt_and_unpinning_gets_a_fresh_grace() {
    let now = Instant::now();
    for event in [Event::Wake, Event::Changed, Event::PinsChanged] {
        let hidden = step(
            Policy::default(),
            false,
            Pins::default(),
            Event::Changed,
            now,
        )
        .unwrap();
        let attempt = step(
            hidden.next,
            false,
            Pins::default(),
            Event::Timer(hidden.next.generation),
            now + SUSPEND_AFTER,
        )
        .unwrap();
        let pins = Pins {
            agent: true,
            ..Pins::default()
        };
        let active = step(attempt.next, false, pins, event, now + SUSPEND_AFTER).unwrap();
        assert!(
            active.resume,
            "a pending native suspend may already have committed"
        );
        assert!(
            step(
                active.next,
                false,
                pins,
                Event::Completed(attempt.next.generation, true),
                now + SUSPEND_AFTER,
            )
            .is_none()
        );
        let released = step(
            active.next,
            false,
            Pins::default(),
            Event::Changed,
            now + SUSPEND_AFTER + Duration::from_secs(1),
        )
        .unwrap();
        assert_eq!(released.sleep, Some(SUSPEND_AFTER));
        let retry = step(
            released.next,
            false,
            Pins::default(),
            Event::Timer(released.next.generation),
            now + SUSPEND_AFTER * 2 + Duration::from_secs(1),
        )
        .unwrap();
        assert!(retry.suspend);
    }
}
