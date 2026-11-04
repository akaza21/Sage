//! Serialize switch reconfiguration and close admission across its entire
//! revoke/write transition. Policies created during it cannot become live.
use std::sync::{Mutex, MutexGuard, atomic::{AtomicU64, Ordering}};

#[derive(Default)]
struct Switch {
    generation: AtomicU64,
    serial: Mutex<()>,
}

static SWITCH: Switch = Switch {
    generation: AtomicU64::new(0),
    serial: Mutex::new(()),
};

pub(crate) fn switch_generation() -> u64 { SWITCH.generation.load(Ordering::SeqCst) }

pub(crate) fn switch_is_current(generation: u64) -> bool { SWITCH.is_current(generation) }

pub(crate) fn with_switch_change<T>(change: impl FnOnce() -> T) -> T {
    let _transition = SWITCH.begin();
    change()
}

impl Switch {
    fn is_current(&self, generation: u64) -> bool {
        generation.is_multiple_of(2)
            && self.generation.load(Ordering::SeqCst) == generation
    }

    fn begin(&self) -> Transition<'_> {
        let serial = self.serial.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        // Odd epochs close admission; every completed transition is even.
        self.generation.fetch_add(1, Ordering::SeqCst);
        Transition { switch: self, _serial: serial }
    }
}

struct Transition<'a> {
    switch: &'a Switch,
    _serial: MutexGuard<'a, ()>,
}

impl Drop for Transition<'_> {
    fn drop(&mut self) {
        self.switch.generation.fetch_add(1, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_switch_writes_deny_old_and_new_approvals_and_failure_retires_both() {
        let switch = Switch::default();
        assert!(switch.is_current(0));
        let transition = switch.begin();
        let during = switch.generation.load(Ordering::SeqCst);
        assert!(!switch.is_current(0));
        assert!(!switch.is_current(during));
        // Both successful and failed writes leave through the same guard.
        drop(transition);
        assert!(!switch.is_current(0));
        assert!(!switch.is_current(during));
        assert!(switch.is_current(switch.generation.load(Ordering::SeqCst)));
    }
}
