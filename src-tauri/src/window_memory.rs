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

#[cfg(windows)]
pub(crate) use native::handle_window_event;

#[cfg(not(windows))]
pub(crate) fn handle_window_event(
    _app: &tauri::AppHandle,
    _label: &str,
    _event: &tauri::WindowEvent,
) {
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

    use super::{MemoryTarget, next_memory_target};

    thread_local! {
        static LAST_TARGETS: RefCell<BTreeMap<String, MemoryTarget>> = const { RefCell::new(BTreeMap::new()) };
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
                apply_memory_target(&webview, label, target);
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
    use super::{MemoryTarget, next_memory_target};

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
