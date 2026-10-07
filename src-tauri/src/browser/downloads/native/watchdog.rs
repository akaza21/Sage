use std::time::{Duration, Instant};

use windows::Win32::{
    Foundation::HWND,
    UI::WindowsAndMessaging::{KillTimer, SetTimer},
};

use super::{PENDING, complete_choice};

const PICKER_TIMEOUT: Duration = Duration::from_secs(5 * 60);

fn accepted_timer_id(timer_id: usize) -> Option<usize> {
    (timer_id != 0).then_some(timer_id)
}

pub(super) fn deadline_reached(armed_at: Instant, now: Instant) -> bool {
    now.saturating_duration_since(armed_at) >= PICKER_TIMEOUT
}

pub(super) fn arm() -> Option<(usize, Instant)> {
    let armed_at = Instant::now();
    accepted_timer_id(unsafe { SetTimer(None, 0, PICKER_TIMEOUT.as_millis() as u32, Some(expire)) })
        .map(|timer_id| (timer_id, armed_at))
}

pub(super) fn disarm(timer_id: usize) {
    let _ = unsafe { KillTimer(None, timer_id) };
}

unsafe extern "system" fn expire(_: HWND, _: u32, timer_id: usize, _: u32) {
    let now = Instant::now();
    let expired = PENDING.with_borrow(|pending| {
        pending.iter().find_map(|(id, entry)| {
            (entry.timer_id == timer_id && deadline_reached(entry.armed_at, now))
                .then(|| (entry.app.clone(), entry.tab_id.clone(), *id))
        })
    });
    if let Some((app, tab_id, id)) = expired {
        complete_choice(&app, &tab_id, id, None);
    }
}

#[cfg(test)]
mod tests {
    use super::{PICKER_TIMEOUT, accepted_timer_id, deadline_reached};
    use std::time::{Duration, Instant};

    #[test]
    fn picker_without_native_deadline_is_denied() {
        assert_eq!(accepted_timer_id(0), None);
        assert_eq!(accepted_timer_id(12), Some(12));
    }

    #[test]
    fn reused_timer_id_cannot_expire_a_new_picker_early() {
        let armed_at = Instant::now();
        assert!(!deadline_reached(
            armed_at,
            armed_at + Duration::from_secs(1)
        ));
        assert!(deadline_reached(armed_at, armed_at + PICKER_TIMEOUT));
    }
}
