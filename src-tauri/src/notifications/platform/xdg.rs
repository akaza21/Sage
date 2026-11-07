//! XDG notices over the session bus. Each notice is sent from its own private
//! connection, which then listens for that notice's click or close. The
//! dispatch slot is released as soon as the notice is shown. At most
//! `MAX_IN_FLIGHT` notices keep a click route, each for `CLICK_WINDOW`: the
//! expiry sent to the server is only a hint, and servers such as GNOME Shell
//! may never report a close, so no route can hold a thread forever. A notice
//! whose route ended stays on screen; clicking it no longer opens its chat.
use super::super::{Event, Notifications, Permit, clicked, should_show};
use dbus::{
    arg::PropMap,
    blocking::Connection,
    message::{MatchRule, Message},
};
use std::{
    sync::{
        Arc, OnceLock,
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
                if !should_show(&app, id) {
                    return;
                }
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
            if shown.wait(CLICK_WINDOW) == Some(Response::Clicked) {
                clicked(&app, id);
            }
        });
    if result.is_err() {
        tracing::warn!("turn notification worker unavailable");
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Response {
    Clicked,
    Closed,
}

/// A shown notice and the private connection that sent it. Servers address
/// signals by notice id, so the connection only reacts to its own notice.
struct Shown {
    bus: Connection,
    response: Arc<OnceLock<Response>>,
}

impl Shown {
    fn send(bus: Connection, event: Event) -> Result<Self, dbus::Error> {
        let notice = Arc::new(AtomicU32::new(0));
        let response = Arc::new(OnceLock::new());
        // Match before sending: the server may report the notice at once.
        let (seen, record) = (notice.clone(), response.clone());
        bus.add_match(
            MatchRule::new_signal(SERVICE, "ActionInvoked").with_path(PATH),
            move |(id, action): (u32, String), _: &Connection, _: &Message| {
                if id == seen.load(Ordering::Acquire) && action == "default" {
                    let _ = record.set(Response::Clicked);
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
        Ok(Self { bus, response })
    }

    /// Waits for this notice's click or close, never past `window`.
    fn wait(self, window: Duration) -> Option<Response> {
        let deadline = Instant::now() + window;
        loop {
            if let Some(response) = self.response.get() {
                return Some(*response);
            }
            let remaining = deadline.checked_duration_since(Instant::now())?;
            if remaining.is_zero() {
                return None;
            }
            self.bus.process(remaining).ok()?;
        }
    }
}

#[cfg(test)]
mod tests;
