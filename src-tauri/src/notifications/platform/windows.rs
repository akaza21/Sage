//! Retain at most four native toast handlers, including after banner dismissal
//! into Action Center. Eviction/exit withdraws the toast and unregisters it.
use super::super::{Event, MAX_IN_FLIGHT, Notifications, Permit, clicked};
use tauri::Manager;
use windows::{
    Data::Xml::Dom::XmlDocument,
    Foundation::TypedEventHandler,
    UI::Notifications::{ToastNotification, ToastNotificationManager},
    core::{HSTRING, IInspectable},
};

pub(in crate::notifications) struct ActiveToast {
    id: i64,
    toast: ToastNotification,
    token: i64,
    app_id: HSTRING,
}
impl Drop for ActiveToast {
    fn drop(&mut self) {
        let _ = self.toast.RemoveActivated(self.token);
        if let Ok(notifier) = ToastNotificationManager::CreateToastNotifierWithId(&self.app_id) {
            let _ = notifier.Hide(&self.toast);
        }
        if let Ok(history) = ToastNotificationManager::History() {
            let _ = history.RemoveGroupedTagWithId(
                &HSTRING::from(self.id.to_string()),
                &HSTRING::from("turns"),
                &self.app_id,
            );
        }
    }
}

pub(in crate::notifications) fn dispatch(
    app: tauri::AppHandle,
    id: i64,
    event: Event,
    permit: Permit,
) {
    let target = app.clone();
    let result = app.run_on_main_thread(move || {
        let _permit = permit;
        if show(&target, id, event).is_err() {
            tracing::warn!("platform turn notification dispatch failed");
        }
    });
    if result.is_err() {
        tracing::warn!("turn notification dispatcher unavailable");
    }
}

fn show(app: &tauri::AppHandle, id: i64, event: Event) -> windows::core::Result<()> {
    let xml = XmlDocument::new()?;
    // All XML content is fixed product text; no harness/chat content enters it.
    xml.LoadXml(&HSTRING::from(format!("<toast><visual><binding template=\"ToastGeneric\"><text>Sage</text><text>{}</text></binding></visual></toast>",event.body())))?;
    let toast = ToastNotification::CreateToastNotification(&xml)?;
    toast.SetTag(&HSTRING::from(id.to_string()))?;
    toast.SetGroup(&HSTRING::from("turns"))?;
    let target = app.clone();
    let token = toast.Activated(&TypedEventHandler::<ToastNotification, IInspectable>::new(
        move |_, _| {
            clicked(&target, id);
            if let Some(state) = target.try_state::<Notifications>() {
                state
                    .toasts
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .retain(|toast| toast.id != id);
            }
            Ok(())
        },
    ))?;
    let app_id = HSTRING::from(app.config().identifier.as_str());
    let retained = ActiveToast {
        id,
        toast,
        token,
        app_id,
    };
    let notifier = ToastNotificationManager::CreateToastNotifierWithId(&retained.app_id)?;
    // Serialize admission before Show, which may synchronously report failure.
    let state = app.state::<Notifications>();
    let mut toasts = state
        .toasts
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if toasts.len() == MAX_IN_FLIGHT {
        toasts.remove(0);
    }
    notifier.Show(&retained.toast)?;
    toasts.push(retained);
    Ok(())
}
