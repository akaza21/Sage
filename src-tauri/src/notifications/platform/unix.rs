use super::super::{Event, Permit, clicked, should_show};

/// No backlog: the caller reserves one of four slots before spawning. Platform
/// waits hold only an opaque receipt, generic text and the shared app handle.
/// 512 KiB stack per worker, released on dismissal, click or dispatch error.
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
            if deliver(event, || clicked(&app, id)).is_err() {
                tracing::warn!("platform turn notification dispatch failed");
            }
        });
    if result.is_err() {
        tracing::warn!("turn notification worker unavailable");
    }
}

fn deliver(event: Event, clicked: impl Fn()) -> notify_rust::error::Result<()> {
    let mut notification = notify_rust::Notification::new();
    notification
        .summary("Sage")
        .body(event.body())
        .appname("Sage")
        .timeout(10_000);
    #[cfg(not(target_os = "macos"))]
    notification.action("default", "Open chat");
    notification.show()?.wait_for_response(|response: &notify_rust::NotificationResponse| {
        if matches!(response,notify_rust::NotificationResponse::Default)
            || matches!(response,notify_rust::NotificationResponse::Action(action) if action=="default") {
            clicked();
        }
    })
}

#[cfg(all(test, target_os = "linux"))]
mod tests;
