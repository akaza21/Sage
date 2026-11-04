#[cfg(any(windows, test))]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MemoryTarget {
    Low,
    Normal,
}

#[cfg(any(windows, test))]
fn next_memory_target(minimized: bool, last: Option<MemoryTarget>) -> Option<MemoryTarget> {
    let target = if minimized {
        MemoryTarget::Low
    } else {
        MemoryTarget::Normal
    };
    (last != Some(target)).then_some(target)
}

#[cfg(any(windows, test))]
fn created_webview_target(minimized: bool) -> Option<MemoryTarget> {
    minimized.then_some(MemoryTarget::Low)
}

#[cfg(windows)]
pub(crate) use native::handle_window_event;

#[cfg(not(windows))]
pub(crate) fn handle_window_event(
    _app: &tauri::AppHandle,
    _label: &str,
    _event: &tauri::WindowEvent,
) {
}

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder = tauri::plugin::Builder::new("window-memory");
    #[cfg(windows)]
    let builder = builder.on_webview_ready(native::handle_webview_ready);
    builder.build()
}

#[cfg(windows)]
mod native {
    use std::{cell::RefCell, collections::BTreeMap};

    use tauri::{AppHandle, Manager, Webview, WindowEvent};
    use tracing::{debug, warn};
    use webview2_com::Microsoft::Web::WebView2::Win32::{
        COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
        ICoreWebView2_19,
    };
    use windows_core::Interface;

    use super::{MemoryTarget, created_webview_target, next_memory_target};

    thread_local! {
        static LAST_TARGETS: RefCell<BTreeMap<String, MemoryTarget>> = const { RefCell::new(BTreeMap::new()) };
    }

    pub(super) fn handle_webview_ready(webview: Webview) {
        let window = webview.window();
        let label = window.label();
        let minimized = match window.is_minimized() {
            Ok(minimized) => minimized,
            Err(error) => {
                warn!(window = label, ?error, "window memory state unavailable");
                return;
            }
        };
        if let Some(target) = created_webview_target(minimized) {
            LAST_TARGETS.with_borrow_mut(|targets| targets.insert(label.to_owned(), target));
            apply_memory_target(&webview, label, target);
        }
    }

    pub(crate) fn handle_window_event(app: &AppHandle, label: &str, event: &WindowEvent) {
        if matches!(event, WindowEvent::Destroyed) {
            LAST_TARGETS.with_borrow_mut(|targets| targets.remove(label));
            return;
        }
        if !matches!(event, WindowEvent::Resized(_)) {
            return;
        }
        let Some(window) = app.get_window(label) else {
            return;
        };
        let minimized = match window.is_minimized() {
            Ok(minimized) => minimized,
            Err(error) => {
                warn!(window = label, ?error, "window memory state unavailable");
                return;
            }
        };
        let target = LAST_TARGETS.with_borrow_mut(|targets| {
            let target = next_memory_target(minimized, targets.get(label).copied())?;
            targets.insert(label.to_owned(), target);
            Some(target)
        });
        if let Some(target) = target {
            for webview in window.webviews() {
                let browser_tab = app
                    .try_state::<crate::browser::BrowserState>()
                    .is_some_and(|state| state.refresh_tab_memory(&webview));
                if target == MemoryTarget::Low || !browser_tab {
                    apply_memory_target(&webview, label, target);
                }
            }
        }
    }

    fn clear_failed_target(label: &str, target: MemoryTarget) {
        LAST_TARGETS.with_borrow_mut(|targets| {
            if targets.get(label) == Some(&target) {
                targets.remove(label);
            }
        });
    }

    fn apply_memory_target(webview: &Webview, window_label: &str, target: MemoryTarget) {
        let label = webview.label().to_owned();
        let callback_window_label = window_label.to_owned();
        if let Err(error) = webview.with_webview(move |platform| {
            let core = match unsafe { platform.controller().CoreWebView2() } {
                Ok(core) => core,
                Err(error) => {
                    clear_failed_target(&callback_window_label, target);
                    warn!(
                        webview = label,
                        ?error,
                        "WebView2 memory controller unavailable"
                    );
                    return;
                }
            };
            let Ok(core) = core.cast::<ICoreWebView2_19>() else {
                debug!(webview = label, "WebView2 runtime lacks memory targets");
                return;
            };
            let level = match target {
                MemoryTarget::Low => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
                MemoryTarget::Normal => COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
            };
            if let Err(error) = unsafe { core.SetMemoryUsageTargetLevel(level) } {
                clear_failed_target(&callback_window_label, target);
                warn!(
                    webview = label,
                    ?error,
                    "WebView2 memory target update failed"
                );
            }
        }) {
            clear_failed_target(window_label, target);
            warn!(
                webview = webview.label(),
                ?error,
                "WebView2 memory target dispatch failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{MemoryTarget, created_webview_target, next_memory_target};

    #[test]
    fn webview_created_after_minimize_gets_low_despite_cached_window_target() {
        let cached_target = next_memory_target(true, None);
        assert_eq!(cached_target, Some(MemoryTarget::Low));
        assert_eq!(next_memory_target(true, cached_target), None);
        assert_eq!(created_webview_target(true), Some(MemoryTarget::Low));
        assert_eq!(created_webview_target(false), None);
        assert_eq!(
            next_memory_target(false, cached_target),
            Some(MemoryTarget::Normal)
        );
    }

    #[test]
    fn minimized_windows_request_low_memory() {
        assert_eq!(next_memory_target(true, None), Some(MemoryTarget::Low));
        assert_eq!(
            next_memory_target(true, Some(MemoryTarget::Normal)),
            Some(MemoryTarget::Low)
        );
    }

    #[test]
    fn visible_windows_request_normal_memory() {
        assert_eq!(next_memory_target(false, None), Some(MemoryTarget::Normal));
        assert_eq!(
            next_memory_target(false, Some(MemoryTarget::Low)),
            Some(MemoryTarget::Normal)
        );
    }

    #[test]
    fn repeated_window_states_do_not_reapply_memory_targets() {
        assert_eq!(next_memory_target(true, Some(MemoryTarget::Low)), None);
        assert_eq!(next_memory_target(false, Some(MemoryTarget::Normal)), None);
    }
}
