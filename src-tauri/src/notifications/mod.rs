//! Turn notices observe committed truth independently of WebView projections.
//! Only commits that may admit a notice reach `observe`, which never blocks:
//! it hands the commit to one notice thread, which claims receipts and
//! dispatches them. Four dispatch slots that end once a notice is shown, no
//! history, and two hashed visible chat IDs. The preference is cached here,
//! so streamed content never touches notice storage.
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
    Arc, Mutex, OnceLock,
    atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering},
    mpsc::{self, SyncSender, TrySendError},
};
use tauri::{Emitter, Manager};

const MAX_IN_FLIGHT: usize = 4;
/// Commits waiting for the notice thread. A claim covers every pending
/// receipt of its session, so a commit dropped from a full queue only delays
/// its receipts to that session's next notice commit.
const MAX_OBSERVED: usize = 16;
const OPEN_EVENT: &str = "sage-notification-open-chat";

#[derive(Default)]
pub(crate) struct Notifications {
    /// Mirrors the durable preference; written only by `start` and the
    /// preference command, so publications never read settings.
    enabled: AtomicBool,
    visibility: Mutex<visibility::Visibility>,
    /// The notice thread's queue, started with the first observed commit.
    observed: OnceLock<Option<SyncSender<ProjectionChangedV1>>>,
    in_flight: Arc<AtomicUsize>,
    /// Shown XDG notices still listening for a click; see `platform::xdg`.
    #[cfg(target_os = "linux")]
    click_routes: Arc<AtomicUsize>,
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
                    #[cfg(target_os = "macos")]
                    platform::withdraw_all();
            }
        }).build()
}

/// Loads the preference once the app state exists and retires receipts left
/// by an earlier run, so a notice never describes another process's event.
pub(crate) fn start(app: &tauri::AppHandle) {
    let (Some(state), Some(notifications)) = (
        app.try_state::<AppState>(),
        app.try_state::<Notifications>(),
    ) else {
        return;
    };
    if receipts::retire_pending(&state.database.connection()).is_err() {
        tracing::warn!("stale turn notification receipts could not be retired");
    }
    let enabled = state.settings.notification_preference().ok()
        == Some(NotificationPreference::BackgroundTurns);
    notifications.enabled.store(enabled, Ordering::Release);
}

impl Notifications {
    /// Applies a saved preference. Turning notices on publishes the flag before
    /// retiring receipts admitted while they were off, so none is lost or
    /// replayed from before the switch.
    fn apply(&self, connection: &rusqlite::Connection, value: NotificationPreference) {
        let enabled = value == NotificationPreference::BackgroundTurns;
        if self.enabled.swap(enabled, Ordering::AcqRel) || !enabled {
            return;
        }
        if receipts::retire_pending(connection).is_err() {
            tracing::warn!("turn notification receipts could not be retired");
        }
    }

    fn enabled(&self) -> bool {
        self.enabled.load(Ordering::Acquire)
    }
}

/// Runs on the publishing thread, often an async ACP pump, so it never
/// touches storage: the commit is queued for the notice thread.
pub(crate) fn observe(app: &tauri::AppHandle, notice: &ProjectionChangedV1) {
    let Some(notifications) = app.try_state::<Notifications>() else {
        return;
    };
    if !notifications.enabled() {
        return;
    }
    let Some(queue) = notifications
        .observed
        .get_or_init(|| start_notice_thread(app.clone()))
    else {
        return;
    };
    match queue.try_send(notice.clone()) {
        Ok(()) | Err(TrySendError::Disconnected(_)) => {}
        Err(TrySendError::Full(_)) => {
            tracing::debug!("turn notice queue full; pending receipts wait for the next commit");
        }
    }
}

fn start_notice_thread(app: tauri::AppHandle) -> Option<SyncSender<ProjectionChangedV1>> {
    let (sender, receiver) = mpsc::sync_channel::<ProjectionChangedV1>(MAX_OBSERVED);
    let started = std::thread::Builder::new()
        .name("turn-notices".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            for notice in receiver {
                announce(&app, &notice);
            }
        });
    if started.is_err() {
        tracing::warn!("turn notification thread unavailable");
        return None;
    }
    Some(sender)
}

/// Claims the commit's receipts and dispatches each one still worth showing.
/// Never retry an uncertain OS delivery; dispatch failure cannot fail a Turn.
fn announce(app: &tauri::AppHandle, notice: &ProjectionChangedV1) {
    let notifications = app.state::<Notifications>();
    if !notifications.enabled() {
        return;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(claims) = receipts::claim(&state.database.connection(), notice) else {
        tracing::warn!("turn notification receipt failed");
        return;
    };
    for (id, event) in claims {
        let Some(permit) = Permit::acquire(&notifications.in_flight) else {
            // The receipt stays claimed: a notice is never replayed late.
            tracing::warn!("turn notification skipped: every dispatch slot is busy");
            continue;
        };
        if should_show(app, id) {
            platform::dispatch(app.clone(), id, event, permit);
        }
    }
}

fn should_show(app: &tauri::AppHandle, id: i64) -> bool {
    let notifications = app.state::<Notifications>();
    if !notifications.enabled() {
        return false;
    }
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let Ok(Some(target)) = receipts::target(&state.database.connection(), id) else {
        return false;
    };
    let Some(window) = app.get_webview_window("main") else {
        return false;
    };
    let focused = window.is_focused().unwrap_or(true);
    let visible = window.is_visible().unwrap_or(true) && !window.is_minimized().unwrap_or(false);
    !notifications
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
