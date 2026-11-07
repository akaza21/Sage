use super::{MemoryState, MemoryTarget};

#[test]
fn hidden_pages_stay_low_through_minimize_and_restore() {
    for minimized in [false, true, false] {
        let state = MemoryState::for_visibility(false, minimized);
        assert_eq!(state, MemoryState::Hidden);
        assert_eq!(state.target(), MemoryTarget::Low);
    }
}

#[test]
fn placed_pages_return_to_normal_on_window_restore() {
    let targets =
        [false, true, false].map(|minimized| MemoryState::for_visibility(true, minimized).target());
    assert_eq!(
        targets,
        [
            MemoryTarget::Normal,
            MemoryTarget::Low,
            MemoryTarget::Normal
        ]
    );
}

#[test]
fn tab_switch_changes_memory_targets_immediately() {
    let targets =
        [true, false, true].map(|placed| MemoryState::for_visibility(placed, false).target());
    assert_eq!(
        targets,
        [
            MemoryTarget::Normal,
            MemoryTarget::Low,
            MemoryTarget::Normal
        ]
    );
}
