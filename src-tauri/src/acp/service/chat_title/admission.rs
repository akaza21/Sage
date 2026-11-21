use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct TitleAdmission {
    state: Mutex<State>,
}
#[derive(Default)]
struct State {
    busy: bool,
    foreground: u64,
    active_turns: usize,
}
impl TitleAdmission {
    fn state(&self) -> std::sync::MutexGuard<'_, State> {
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
    pub(crate) fn foreground(&self) {
        self.state().foreground += 1;
    }
    pub(super) fn turn(self: &Arc<Self>) -> ForegroundLease {
        let mut state = self.state();
        state.foreground += 1;
        state.active_turns += 1;
        ForegroundLease(Arc::clone(self))
    }
    pub(super) fn generation(&self) -> u64 {
        self.state().foreground
    }
    pub(super) fn acquire(self: &Arc<Self>) -> Option<(Lease, u64)> {
        let mut state = self.state();
        if state.busy || state.active_turns != 0 {
            return None;
        }
        state.busy = true;
        Some((Lease(Arc::clone(self)), state.foreground))
    }
}

pub(crate) struct ForegroundLease(Arc<TitleAdmission>);
impl Drop for ForegroundLease {
    fn drop(&mut self) {
        self.0.state().active_turns -= 1;
    }
}
pub(super) struct Lease(Arc<TitleAdmission>);
impl Drop for Lease {
    fn drop(&mut self) {
        self.0.state().busy = false;
    }
}
