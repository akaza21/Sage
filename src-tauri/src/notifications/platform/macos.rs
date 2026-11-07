//! macOS notices through `NSUserNotificationCenter`. notify-rust can only
//! report a click by blocking until the user responds, which may be never,
//! so a notice is sent without waiting and the dispatch slot is released at
//! once. Clicking it brings Sage forward but does not choose the chat.
use super::super::{Event, Permit, should_show};

pub(in crate::notifications) fn dispatch(
    app: tauri::AppHandle,
    id: i64,
    event: Event,
    permit: Permit,
) {
    let result = std::thread::Builder::new()
        .name("turn-notice".into())
        .stack_size(512 * 1024)
        .spawn(move || {
            let _permit = permit;
            if !should_show(&app, id) {
                return;
            }
            // Dropping the handle sends the notice asynchronously.
            if notify_rust::Notification::new()
                .summary("Sage")
                .body(event.body())
                .show()
                .is_err()
            {
                tracing::warn!("platform turn notification dispatch failed");
            }
        });
    if result.is_err() {
        tracing::warn!("turn notification worker unavailable");
    }
}
