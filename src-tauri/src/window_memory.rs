//! Window events are inputs to the shared per-WebView memory policy.
#[cfg(any(windows, test))]
use std::{cell::RefCell, collections::BTreeMap};

#[cfg(windows)]
use tauri::Manager;

#[cfg(any(windows, test))]
use crate::browser::background::{self, MemoryTargetPort};

#[cfg(any(windows, test))]
trait MemoryWindow {
    type Webview: MemoryTargetPort;

    fn is_minimized(&self) -> Result<bool, String>;
    fn webviews(&self) -> Vec<Self::Webview>;
}

#[cfg(any(windows, test))]
#[derive(Default)]
struct WindowMemory {
    last_minimized: RefCell<BTreeMap<String, bool>>,
}

#[cfg(any(windows, test))]
impl WindowMemory {
    fn handle_webview_ready(
        &self,
        label: &str,
        window: &impl MemoryWindow,
        webview: &impl MemoryTargetPort,
    ) {
        if let Ok(minimized) = window.is_minimized() {
            // Creation seeds a new window, but must not consume a transition
            // that its existing WebViews have yet to observe.
            self.last_minimized
                .borrow_mut()
                .entry(label.to_owned())
                .or_insert(minimized);
        }
        background::refresh(webview);
    }

    fn handle_window_event<W: MemoryWindow>(
        &self,
        label: &str,
        event: &tauri::WindowEvent,
        window: impl FnOnce() -> Option<W>,
    ) {
        if matches!(event, tauri::WindowEvent::Destroyed) {
            self.last_minimized.borrow_mut().remove(label);
            return;
        }
        if !matches!(event, tauri::WindowEvent::Resized(_)) {
            return;
        }
        let Some(window) = window() else {
            return;
        };
        let minimized = match window.is_minimized() {
            Ok(minimized) => minimized,
            Err(error) => {
                tracing::warn!(window = label, %error, "window memory state unavailable");
                return;
            }
        };
        let previous = self
            .last_minimized
            .borrow_mut()
            .insert(label.to_owned(), minimized);
        if previous != Some(minimized) {
            for webview in window.webviews() {
                background::refresh(&webview);
            }
        }
    }
}

#[cfg(windows)]
thread_local! {
    static MEMORY: WindowMemory = const {
        WindowMemory { last_minimized: RefCell::new(BTreeMap::new()) }
    };
}

#[cfg(windows)]
impl MemoryWindow for tauri::Window {
    type Webview = tauri::Webview;

    fn is_minimized(&self) -> Result<bool, String> {
        self.is_minimized().map_err(|error| error.to_string())
    }

    fn webviews(&self) -> Vec<Self::Webview> {
        self.webviews()
    }
}

pub(crate) fn handle_window_event(app: &tauri::AppHandle, label: &str, event: &tauri::WindowEvent) {
    #[cfg(windows)]
    MEMORY.with(|memory| memory.handle_window_event(label, event, || app.get_window(label)));
    #[cfg(not(windows))]
    let _ = (app, label, event);
}

pub(crate) fn init() -> tauri::plugin::TauriPlugin<tauri::Wry> {
    let builder = tauri::plugin::Builder::new("window-memory");
    #[cfg(windows)]
    let builder = builder.on_webview_ready(|webview| {
        let window = webview.window();
        MEMORY.with(|memory| memory.handle_webview_ready(window.label(), &window, &webview));
    });
    builder.build()
}

#[cfg(test)]
mod tests;
