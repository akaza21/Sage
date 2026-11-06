use super::{Notifications, receipts, visibility::VisibilityUpdate};
use crate::{settings_store::NotificationPreference, state::AppState};
use std::sync::atomic::Ordering;
use tauri::State;

#[tauri::command]
pub fn get_notification_preference(
    state: State<'_, AppState>,
) -> Result<NotificationPreference, String> {
    state
        .settings
        .notification_preference()
        .map_err(|_| "notification preference unavailable".into())
}
#[tauri::command]
pub fn set_notification_preference(
    state: State<'_, AppState>,
    value: NotificationPreference,
) -> Result<NotificationPreference, String> {
    state
        .settings
        .set_notification_preference(value)
        .map_err(|_| "notification preference could not be saved".into())
}
#[tauri::command]
pub fn begin_notification_visibility(state: State<'_, Notifications>) -> u64 {
    state
        .visibility
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .reset()
}
#[tauri::command]
pub fn report_notification_visibility(
    state: State<'_, Notifications>,
    update: VisibilityUpdate,
) -> Result<(), String> {
    state
        .visibility
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .update(update)
}
#[tauri::command]
pub fn take_notification_chat(
    state: State<'_, AppState>,
    notifications: State<'_, Notifications>,
) -> Result<Option<receipts::OpenChat>, String> {
    let id = notifications.pending_open.swap(0, Ordering::AcqRel);
    if id == 0 {
        return Ok(None);
    }
    receipts::target(&state.database.connection(), id)
        .map_err(|_| "notification chat unavailable".into())
}
