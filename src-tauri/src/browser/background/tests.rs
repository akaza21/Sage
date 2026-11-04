use super::*;

fn step(
    current: Policy,
    placed: bool,
    pins: Pins,
    event: Event,
    now: Instant,
) -> Option<Transition> {
    next_state(
        Inputs {
            current,
            placed,
            minimized: false,
            pins,
            event,
        },
        now,
    )
}

#[test]
fn hidden_pages_get_low_then_suspend_after_the_grace_and_resume_on_placement() {
    let now = Instant::now();
    let hide = step(
        Policy::default(),
        false,
        Pins::default(),
        Event::Changed,
        now,
    )
    .unwrap();
    assert_eq!(hide.target, MemoryTarget::Low);
    assert_eq!(hide.sleep, Some(Duration::from_secs(30)));
    assert!(!hide.suspend);
    assert!(
        step(
            hide.next,
            false,
            Pins::default(),
            Event::Timer(hide.next.generation),
            now + Duration::from_secs(29)
        )
        .is_none()
    );
    let suspend = step(
        hide.next,
        false,
        Pins::default(),
        Event::Timer(hide.next.generation),
        now + Duration::from_secs(30),
    )
    .unwrap();
    assert!(suspend.suspend);
    let done = step(
        suspend.next,
        false,
        Pins::default(),
        Event::Completed(suspend.next.generation, true),
        now + Duration::from_secs(30),
    )
    .unwrap();
    assert_eq!(done.next.state, MemoryState::Suspended);
    let show = step(
        done.next,
        true,
        Pins::default(),
        Event::Changed,
        now + Duration::from_secs(31),
    )
    .unwrap();
    assert!(show.resume);
    assert_eq!(show.target, MemoryTarget::Normal);
}

#[test]
fn each_pin_keeps_a_hidden_page_running_and_unpinning_starts_a_fresh_grace() {
    let now = Instant::now();
    for pins in [
        Pins {
            audio: true,
            ..Pins::default()
        },
        Pins {
            download: true,
            ..Pins::default()
        },
        Pins {
            devtools: true,
            ..Pins::default()
        },
        Pins {
            annotate: true,
            ..Pins::default()
        },
        Pins {
            agent: true,
            ..Pins::default()
        },
        Pins {
            loading: true,
            ..Pins::default()
        },
    ] {
        let hidden = step(Policy::default(), false, pins, Event::Changed, now).unwrap();
        assert_eq!(hidden.target, MemoryTarget::Low);
        assert_eq!(hidden.sleep, None);
        assert!(
            step(
                hidden.next,
                false,
                pins,
                Event::Timer(hidden.next.generation),
                now + Duration::from_secs(60)
            )
            .is_none()
        );
        let released = step(
            hidden.next,
            false,
            Pins::default(),
            Event::Changed,
            now + Duration::from_secs(60),
        )
        .unwrap();
        assert_eq!(released.sleep, Some(Duration::from_secs(30)));
        assert!(!released.suspend);
    }
}

#[test]
fn stale_timers_and_suspend_completions_do_nothing_after_a_tab_is_placed() {
    let now = Instant::now();
    let hidden = step(
        Policy::default(),
        false,
        Pins::default(),
        Event::Changed,
        now,
    )
    .unwrap();
    let shown = step(hidden.next, true, Pins::default(), Event::Changed, now).unwrap();
    assert!(shown.resume);
    for event in [
        Event::Timer(hidden.next.generation),
        Event::Completed(hidden.next.generation, true),
    ] {
        assert!(
            step(
                shown.next,
                true,
                Pins::default(),
                event,
                now + Duration::from_secs(60)
            )
            .is_none()
        );
    }
}

#[test]
fn a_pin_arriving_with_suspend_completion_wakes_the_page() {
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
        now + Duration::from_secs(30),
    )
    .unwrap();
    let pinned = step(
        attempt.next,
        false,
        Pins {
            download: true,
            ..Pins::default()
        },
        Event::Completed(attempt.next.generation, true),
        now + Duration::from_secs(30),
    )
    .unwrap();
    assert!(pinned.resume);
    assert!(matches!(pinned.next.state, MemoryState::Hidden { .. }));
    assert!(!pinned.suspend);
}

#[test]
fn activity_on_a_suspended_page_resumes_it_without_placing_it() {
    let now = Instant::now();
    let suspended = Policy {
        state: MemoryState::Suspended,
        tried: true,
        ..Policy::default()
    };
    let activity = step(
        suspended,
        false,
        Pins {
            audio: true,
            ..Pins::default()
        },
        Event::Changed,
        now,
    )
    .unwrap();
    assert!(activity.resume);
    assert!(matches!(activity.next.state, MemoryState::Hidden { .. }));
    assert_eq!(activity.target, MemoryTarget::Low);
    assert_eq!(activity.sleep, None);
}

#[test]
fn an_operation_wake_resumes_even_before_native_suspend_completion_reaches_the_policy() {
    let now = Instant::now();
    let wake = step(Policy::default(), true, Pins::default(), Event::Wake, now).unwrap();
    assert!(wake.resume);
    assert_eq!(wake.target, MemoryTarget::Normal);
    assert_eq!(wake.sleep, None);
    assert!(!wake.suspend);
}

#[test]
fn a_coalesced_pin_change_restarts_hidden_grace_without_waking_a_suspended_page() {
    let now = Instant::now();
    let hidden = step(
        Policy::default(),
        false,
        Pins::default(),
        Event::Changed,
        now,
    )
    .unwrap();
    let changed = step(
        hidden.next,
        false,
        Pins::default(),
        Event::PinsChanged,
        now + Duration::from_secs(29),
    )
    .unwrap();
    assert_eq!(changed.sleep, Some(Duration::from_secs(30)));
    assert!(!changed.resume);
    assert!(!changed.suspend);
    let suspended = Policy {
        state: MemoryState::Suspended,
        tried: true,
        ..Policy::default()
    };
    assert!(step(suspended, false, Pins::default(), Event::PinsChanged, now).is_none());
}

#[test]
fn a_declined_suspend_stays_hidden_without_retrying_until_the_next_hide() {
    let now = Instant::now();
    let hidden = step(
        Policy::default(),
        false,
        Pins::default(),
        Event::Changed,
        now,
    )
    .unwrap();
    let attempted = step(
        hidden.next,
        false,
        Pins::default(),
        Event::Timer(hidden.next.generation),
        now + Duration::from_secs(30),
    )
    .unwrap();
    assert!(
        step(
            attempted.next,
            false,
            Pins::default(),
            Event::Completed(attempted.next.generation, false),
            now + Duration::from_secs(30)
        )
        .is_none()
    );
    assert!(matches!(attempted.next.state, MemoryState::Hidden { .. }));
    assert!(
        step(
            attempted.next,
            false,
            Pins::default(),
            Event::Changed,
            now + Duration::from_secs(90)
        )
        .is_none()
    );
    let placed = step(
        attempted.next,
        true,
        Pins::default(),
        Event::Changed,
        now + Duration::from_secs(90),
    )
    .unwrap();
    let hidden = step(
        placed.next,
        false,
        Pins::default(),
        Event::Changed,
        now + Duration::from_secs(91),
    )
    .unwrap();
    assert_eq!(hidden.sleep, Some(Duration::from_secs(30)));
}

#[test]
fn minimized_window_keeps_a_placed_page_low_and_wake_preserves_hidden_placement() {
    let now = Instant::now();
    let minimized = next_state(
        Inputs {
            current: Policy::default(),
            placed: true,
            minimized: true,
            pins: Pins::default(),
            event: Event::Changed,
        },
        now,
    )
    .unwrap();
    assert_eq!(minimized.target, MemoryTarget::Low);
    assert_eq!(minimized.sleep, None);
    let restored = step(minimized.next, true, Pins::default(), Event::Changed, now).unwrap();
    assert_eq!(restored.target, MemoryTarget::Normal);
    let hidden = step(restored.next, false, Pins::default(), Event::Changed, now).unwrap();
    let attempted = step(
        hidden.next,
        false,
        Pins::default(),
        Event::Timer(hidden.next.generation),
        now + Duration::from_secs(30),
    )
    .unwrap();
    let suspended = step(
        attempted.next,
        false,
        Pins::default(),
        Event::Completed(attempted.next.generation, true),
        now + Duration::from_secs(30),
    )
    .unwrap();
    let wake = step(
        suspended.next,
        false,
        Pins {
            agent: true,
            ..Pins::default()
        },
        Event::Wake,
        now + Duration::from_secs(31),
    )
    .unwrap();
    assert!(wake.resume);
    assert!(matches!(wake.next.state, MemoryState::Hidden { .. }));
    assert_eq!(wake.target, MemoryTarget::Low);
}
