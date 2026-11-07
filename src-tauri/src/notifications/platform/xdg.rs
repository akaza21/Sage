//! XDG notices over the session bus. Each notice is sent from its own private
//! connection, which then listens for that notice's click or close. The
//! dispatch slot is released as soon as the notice is shown. At most
//! `MAX_IN_FLIGHT` notices keep a click route, each for `CLICK_WINDOW`: the
//! expiry sent to the server is only a hint, and servers such as GNOME Shell
//! may never report a close, so no route can hold a thread forever. A notice
//! whose route ends is withdrawn, so no notice is left that cannot open its
//! chat. A click carries the activation token the server sends just before
//! it, so Wayland compositors let Sage raise its window.
use super::super::{Event, Notifications, Permit, clicked};
use dbus::{
    arg::PropMap,
    blocking::Connection,
    message::{MatchRule, Message},
};
use std::{
    ffi::CString,
    sync::{
        Arc, Mutex, OnceLock, PoisonError,
        atomic::{AtomicU32, Ordering},
    },
    time::{Duration, Instant},
};
use tauri::Manager;

const SERVICE: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const CALL_TIMEOUT: Duration = Duration::from_secs(2);
const EXPIRE_MS: i32 = 10_000;
const CLICK_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Tokens are short opaque strings; anything longer is not kept.
const MAX_TOKEN_BYTES: usize = 256;

pub(in crate::notifications) fn dispatch(
    app: tauri::AppHandle,
    id: i64,
    event: Event,
    permit: Permit,
) {
    let routes = app.state::<Notifications>().click_routes.clone();
    let result = std::thread::Builder::new()
        .name("turn-notice".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let shown = {
                let _permit = permit;
                match Connection::new_session().and_then(|bus| Shown::send(bus, event)) {
                    Ok(shown) => shown,
                    Err(_) => {
                        tracing::warn!("platform turn notification dispatch failed");
                        return;
                    }
                }
            };
            let Some(_route) = Permit::acquire(&routes) else {
                tracing::debug!("turn notification shown without a click route");
                return;
            };
            match shown.wait(CLICK_WINDOW) {
                Some(Response::Clicked(activation)) => clicked(&app, id, activation),
                Some(Response::Closed) => {}
                None => shown.withdraw(),
            }
        });
    if result.is_err() {
        tracing::warn!("turn notification worker unavailable");
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Response {
    /// Clicked, with the activation token the server sent for the click.
    Clicked(Option<String>),
    Closed,
}

/// Raises the window with a click's activation token. GTK spends the token
/// on its next focus request, which Wayland compositors otherwise refuse
/// without a recent input event; on X11 it is a plain focus request.
pub(in crate::notifications) fn present(window: &tauri::WebviewWindow, token: String) {
    use gtk::prelude::*;
    let target = window.clone();
    let presented = window.run_on_main_thread(move || {
        let Ok(gtk_window) = target.gtk_window() else {
            return;
        };
        let display = gtk_window.display();
        if display.type_().name() == "GdkWaylandDisplay"
            && let Ok(token) = CString::new(token)
        {
            // SAFETY: on the main thread, `display` is a live
            // GdkWaylandDisplay (checked by type name); GDK copies the id.
            unsafe {
                gdk_wayland_sys::gdk_wayland_display_set_startup_notification_id(
                    display.as_ptr().cast(),
                    token.as_ptr(),
                );
            }
        }
        gtk_window.present_with_time(gtk::gdk::ffi::GDK_CURRENT_TIME as u32);
    });
    if presented.is_err() {
        let _ = window.set_focus();
    }
}

/// A shown notice and the private connection that sent it. Servers address
/// signals by notice id, so the connection only reacts to its own notice.
struct Shown {
    bus: Connection,
    notice: Arc<AtomicU32>,
    response: Arc<OnceLock<Response>>,
}

impl Shown {
    fn send(bus: Connection, event: Event) -> Result<Self, dbus::Error> {
        let notice = Arc::new(AtomicU32::new(0));
        let response = Arc::new(OnceLock::new());
        let token = Arc::new(Mutex::new(None::<String>));
        // Match before sending: the server may report the notice at once.
        let (seen, granted) = (notice.clone(), token.clone());
        bus.add_match(
            MatchRule::new_signal(SERVICE, "ActivationToken").with_path(PATH),
            move |(id, value): (u32, String), _: &Connection, _: &Message| {
                if id == seen.load(Ordering::Acquire) && value.len() <= MAX_TOKEN_BYTES {
                    *granted.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
                }
                true
            },
        )?;
        let (seen, record) = (notice.clone(), response.clone());
        bus.add_match(
            MatchRule::new_signal(SERVICE, "ActionInvoked").with_path(PATH),
            move |(id, action): (u32, String), _: &Connection, _: &Message| {
                if id == seen.load(Ordering::Acquire) && action == "default" {
                    let activation = token.lock().unwrap_or_else(PoisonError::into_inner).take();
                    let _ = record.set(Response::Clicked(activation));
                }
                true
            },
        )?;
        let (seen, record) = (notice.clone(), response.clone());
        bus.add_match(
            MatchRule::new_signal(SERVICE, "NotificationClosed").with_path(PATH),
            move |(id, _reason): (u32, u32), _: &Connection, _: &Message| {
                if id == seen.load(Ordering::Acquire) {
                    let _ = record.set(Response::Closed);
                }
                true
            },
        )?;
        // All text is fixed product copy; no harness or chat content enters it.
        let (id,): (u32,) = bus.with_proxy(SERVICE, PATH, CALL_TIMEOUT).method_call(
            SERVICE,
            "Notify",
            (
                "Sage",
                0_u32,
                "",
                "Sage",
                event.body(),
                vec!["default", "Open chat"],
                PropMap::new(),
                EXPIRE_MS,
            ),
        )?;
        notice.store(id, Ordering::Release);
        Ok(Self {
            bus,
            notice,
            response,
        })
    }

    /// Waits for this notice's click or close, never past `window`.
    fn wait(&self, window: Duration) -> Option<Response> {
        let deadline = Instant::now() + window;
        loop {
            if let Some(response) = self.response.get() {
                return Some(response.clone());
            }
            let remaining = deadline.checked_duration_since(Instant::now())?;
            if remaining.is_zero() {
                return None;
            }
            self.bus.process(remaining).ok()?;
        }
    }

    /// Removes a notice whose click route ended. Best effort: the server
    /// may already have dropped it.
    fn withdraw(&self) {
        let id = self.notice.load(Ordering::Acquire);
        let closed: Result<(), dbus::Error> = self
            .bus
            .with_proxy(SERVICE, PATH, CALL_TIMEOUT)
            .method_call(SERVICE, "CloseNotification", (id,));
        if closed.is_err() {
            tracing::debug!("expired turn notification could not be withdrawn");
        }
    }
}

#[cfg(test)]
mod tests;
