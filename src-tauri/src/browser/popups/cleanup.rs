use tauri::{AppHandle, Manager};

use super::{BrowserState, BrowserTabs, PopupRequests, PopupState};

impl PopupRequests {
    pub(super) fn cleanup_target(&mut self, id: u64, opener: &str, tab_id: &str) -> bool {
        let Some((intent, state)) = self.entries.get_mut(&id) else {
            return false;
        };
        if intent.opener_tab_id != opener {
            return false;
        }
        match state {
            PopupState::Opened(opened) if opened == tab_id => {
                *state = PopupState::Quarantined(Some(tab_id.to_owned()));
                true
            }
            PopupState::Quarantined(Some(opened)) if opened == tab_id => true,
            _ => false,
        }
    }

    pub(in crate::browser) fn finish_cleanup(&mut self, id: u64, tab_id: &str) {
        if self.entries.get(&id).is_some_and(
            |(_, state)| matches!(state, PopupState::Quarantined(Some(opened)) if opened == tab_id),
        ) {
            self.entries.remove(&id);
            self.native_deadlines.remove(&id);
            self.initial_blank.remove(tab_id);
        }
    }

    fn quarantine_child(&mut self, label: &str) -> Option<u64> {
        let id = self.entries.iter().find_map(|(id, (_, state))| {
            matches!(state, super::PopupState::Opened(child) | super::PopupState::Quarantined(Some(child)) if child == label)
                .then_some(*id)
        })?;
        self.quarantine(id, Some(label.to_owned()));
        Some(id)
    }
}

pub(super) async fn abandon_child(
    app: &AppHandle,
    state: &BrowserState,
    label: &str,
) -> Result<(), String> {
    let id = state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .quarantine_child(label);
    if let Some(id) = id {
        close_popup(app, state, id, label).await
    } else {
        close_child(app, state, label).await
    }
}

pub(in crate::browser) async fn close_popup(
    app: &AppHandle,
    state: &BrowserState,
    id: u64,
    label: &str,
) -> Result<(), String> {
    if let Ok(mut popups) = state.popups.lock() {
        popups.quarantine(id, Some(label.to_owned()));
    }
    close_child(app, state, label).await?;
    state
        .popups
        .lock()
        .map_err(|error| error.to_string())?
        .finish_cleanup(id, label);
    Ok(())
}

async fn close_child(app: &AppHandle, state: &BrowserState, label: &str) -> Result<(), String> {
    if let Some(webview) = state.webview_for_tab(label) {
        let _ = webview.hide();
    }
    close_with_retry(|| async {
        wait_for_closing(&state.tabs, label).await?;
        let gone = state
            .tabs
            .lock()
            .is_ok_and(|tabs| confirmed_closed(&tabs, label));
        if gone {
            return Ok(());
        }
        super::super::lifecycle::close_browser_tab(app, state, label.to_owned()).await
    })
    .await
}

pub(super) async fn wait_for_closing<T>(
    tabs: &std::sync::Mutex<BrowserTabs<T>>,
    label: &str,
) -> Result<(), String> {
    tokio::time::timeout(std::time::Duration::from_secs(10), async {
        loop {
            let closing = tabs
                .lock()
                .map_err(|error| error.to_string())?
                .closing
                .contains(label);
            if !closing {
                return Ok(());
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .map_err(|_| super::super::teardown::UNCERTAIN_CLOSE_PREFIX.to_owned())?
}

pub(super) fn confirmed_closed<T>(tabs: &BrowserTabs<T>, label: &str) -> bool {
    !tabs.entries.contains_key(label)
        && !tabs.closing.contains(label)
        && tabs.teardown.blocked_message().is_none()
}

async fn close_with_retry<Future: std::future::Future<Output = Result<(), String>>>(
    mut close: impl FnMut() -> Future,
) -> Result<(), String> {
    for attempt in 0..3 {
        match close().await {
            Ok(()) => return Ok(()),
            Err(error)
                if attempt == 2
                    || error.starts_with(super::super::teardown::UNCERTAIN_CLOSE_PREFIX) =>
            {
                return Err(error);
            }
            Err(_) => tokio::task::yield_now().await,
        }
    }
    unreachable!()
}

pub(super) fn expire_unacknowledged(app: &AppHandle) {
    let state = app.state::<BrowserState>();
    let scheduled = state.popups.lock().is_ok_and(|mut popups| {
        if popups.cleanup_scheduled || popups.native_deadlines.is_empty() {
            return false;
        }
        popups.cleanup_scheduled = true;
        true
    });
    if !scheduled {
        return;
    }
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        let state = app.state::<BrowserState>();
        loop {
            let deadline = state.popups.lock().ok().and_then(|mut popups| {
                let deadline = popups.native_deadlines.values().min().copied();
                if deadline.is_none() {
                    popups.cleanup_scheduled = false;
                }
                deadline
            });
            let Some(deadline) = deadline else {
                return;
            };
            tokio::time::sleep_until(deadline.into()).await;
            let expired = state
                .popups
                .lock()
                .map(|mut popups| popups.take_expired_native(std::time::Instant::now()))
                .unwrap_or_default();
            for (id, label) in expired {
                if let Err(error) = close_popup(&app, &state, id, &label).await {
                    eprintln!("browser popup deadline close failed: {error}");
                }
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn abandoned_navigation_quarantines_only_its_unacknowledged_child() {
        let mut popups = super::super::PopupRequests::default();
        let first = popups
            .insert(
                "browser-1",
                Some("https://example.com/".into()),
                super::super::PopupState::Opening,
            )
            .unwrap();
        let second = popups
            .insert(
                "browser-1",
                Some("https://example.com/".into()),
                super::super::PopupState::Opening,
            )
            .unwrap();
        assert!(popups.opened(first.id, "browser-2"));
        assert!(popups.opened(second.id, "browser-3"));
        assert_eq!(popups.quarantine_child("browser-2"), Some(first.id));
        assert!(!popups.ack(first.id, "browser-1", "browser-2"));
        assert!(popups.ack(second.id, "browser-1", "browser-3"));
        assert_eq!(popups.quarantine_child("browser-3"), None);
    }

    #[tokio::test]
    async fn overlapping_cleanup_does_not_confirm_a_close_that_later_fails() {
        let tabs = std::sync::Mutex::new(BrowserTabs::<usize>::new());
        tabs.lock().unwrap().insert("browser-2".into(), 2, false);
        let result =
            crate::browser::lifecycle::close_with_unlocked(&tabs, "browser-2", |_| async {
                let tabs = tabs.lock().unwrap();
                assert!(!tabs.entries.contains_key("browser-2"));
                assert!(tabs.closing.contains("browser-2"));
                assert!(!confirmed_closed(&tabs, "browser-2"));
                Err("native detach failed".into())
            })
            .await;
        assert!(result.is_err());
        assert!(tabs.lock().unwrap().entries.contains_key("browser-2"));
        assert!(!confirmed_closed(&tabs.lock().unwrap(), "browser-2"));
    }

    #[test]
    fn late_success_retires_quarantined_child_and_allows_delayed_abandonment() {
        let mut popups = PopupRequests::default();
        let intent = popups
            .insert(
                "browser-1",
                Some("https://example.com/".into()),
                PopupState::Opening,
            )
            .unwrap();
        assert!(popups.opened(intent.id, "browser-2"));
        assert!(popups.cleanup_target(intent.id, "browser-1", "browser-2"));
        let mut tabs = BrowserTabs::<usize>::new();
        let quarantine = tabs.teardown.clone();
        quarantine.begin("browser-2").unwrap();
        quarantine.mark_failed("browser-2");
        assert!(!confirmed_closed(&tabs, "browser-2"));
        quarantine.acknowledge("browser-2", &Ok(()));
        popups.forget_child("browser-2");
        assert!(popups.take_for_opener("browser-1").is_empty());
        assert!(!popups.cleanup_target(intent.id, "browser-1", "browser-2"));
        assert!(confirmed_closed(&tabs, "browser-2"));
        tabs.insert("browser-3".into(), 3, false);
        assert!(!confirmed_closed(&tabs, "browser-3"));
    }

    #[tokio::test]
    async fn failed_popup_close_retries_without_losing_cleanup_ownership() {
        let attempts = Cell::new(0);
        let result = close_with_retry(|| async {
            attempts.set(attempts.get() + 1);
            if attempts.get() == 1 {
                Err("definite close failure".into())
            } else {
                Ok(())
            }
        })
        .await;
        assert_eq!(result, Ok(()));
        assert_eq!(attempts.get(), 2);
    }

    #[tokio::test]
    async fn uncertain_popup_close_is_not_reissued() {
        let attempts = Cell::new(0);
        let result = close_with_retry(|| async {
            attempts.set(attempts.get() + 1);
            Err(crate::browser::teardown::UNCERTAIN_CLOSE_PREFIX.into())
        })
        .await;
        assert!(result.is_err());
        assert_eq!(attempts.get(), 1);
    }
}
