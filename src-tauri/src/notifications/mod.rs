//! Turn notices observe committed truth independently of WebView projections.
//! Four platform waits, no work queue/history; two hashed visible chat IDs.
mod commands;
mod platform;
mod receipts;
mod visibility;

use crate::{
    acp::intent::ProjectionChangedV1, settings_store::NotificationPreference, state::AppState,
};
pub use commands::*;
use receipts::Event;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicI64, AtomicUsize, Ordering},
};
use tauri::{Emitter, Manager};

const MAX_IN_FLIGHT: usize = 4;
const OPEN_EVENT: &str = "sage-notification-open-chat";

#[derive(Default)]
pub(crate) struct Notifications {
    visibility: Mutex<visibility::Visibility>,
    in_flight: Arc<AtomicUsize>,
    pending_open: AtomicI64,
    #[cfg(windows)]
    toasts: Mutex<Vec<platform::ActiveToast>>,
}

struct Permit(Arc<AtomicUsize>);
impl Permit {
    fn acquire(count: &Arc<AtomicUsize>) -> Option<Self> {
        count
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value < MAX_IN_FLIGHT).then_some(value + 1)
            })
            .ok()?;
        Some(Self(count.clone()))
    }
}
impl Drop for Permit {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    tauri::plugin::Builder::new("turn-notifications")
        .setup(|app, _| { app.manage(Notifications::default()); Ok(()) })
        .on_event(|app, event| {
            if (matches!(event, tauri::RunEvent::Exit) || matches!(event, tauri::RunEvent::WindowEvent { label, event: tauri::WindowEvent::Destroyed, .. } if label == "main"))
                && let Some(state) = app.try_state::<Notifications>() {
                    state.visibility.lock().unwrap_or_else(std::sync::PoisonError::into_inner).reset();
                    state.pending_open.store(0, Ordering::Release);
                    #[cfg(windows)]
                    state.toasts.lock().unwrap_or_else(std::sync::PoisonError::into_inner).clear();
            }
        }).build()
}

pub(crate) fn observe(app: &tauri::AppHandle, notice: &ProjectionChangedV1) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    // Synchronous receipt admission, outside the journal's committed transaction.
    // Never retry an uncertain OS delivery; dispatch failure cannot fail a Turn.
    let claims = receipts::claim(&state.database.connection(), notice);
    let Ok(claims) = claims else {
        tracing::warn!("turn notification receipt failed");
        return;
    };
    if state.settings.notification_preference().ok()
        != Some(NotificationPreference::BackgroundTurns)
    {
        return;
    }
    let notifications = app.state::<Notifications>();
    for (id, event) in claims {
        let Some(permit) = Permit::acquire(&notifications.in_flight) else {
            continue;
        };
        platform::dispatch(app.clone(), id, event, permit);
    }
}

fn should_show(app: &tauri::AppHandle, id: i64) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    if state.settings.notification_preference().ok()
        != Some(NotificationPreference::BackgroundTurns)
    {
        return false;
    }
    let Ok(Some(target)) = receipts::target(&state.database.connection(), id) else {
        return false;
    };
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let focused = window.is_focused().unwrap_or(true);
    let visible = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
    !app.state::<Notifications>()
        .visibility
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .suppresses(&target.chat_id, focused && visible)
}

fn clicked(app: &tauri::AppHandle, id: i64) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    if !matches!(
        receipts::target(&state.database.connection(), id),
        Ok(Some(_))
    ) {
        return;
    }
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    app.state::<Notifications>()
        .pending_open
        .store(id, Ordering::Release);
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
    let _ = app.emit(OPEN_EVENT, ());
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn dispatch_capacity_is_bounded_and_released_on_failure_or_completion() {
        let count = Arc::new(AtomicUsize::new(0));
        let permits: Vec<_> = (0..MAX_IN_FLIGHT)
            .map(|_| Permit::acquire(&count).unwrap())
            .collect();
        assert!(Permit::acquire(&count).is_none());
        drop(permits);
        assert_eq!(count.load(Ordering::Acquire), 0);
        assert!(Permit::acquire(&count).is_some());
    }
}
