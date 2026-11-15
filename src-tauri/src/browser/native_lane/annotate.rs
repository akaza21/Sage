use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::Value;

use super::Lane;
use crate::browser::{annotate, bounds, page_controls};

impl Lane {
    pub fn annotate_control(&self, tab: &str, action: &str) -> Result<Option<Value>, String> {
        let action = annotate::action(action).ok_or("unknown annotate action")?;
        tauri::async_runtime::block_on(annotate::set(
            self.app.clone(),
            tab.to_owned(),
            action,
            None,
            None,
        ))
    }

    pub fn pick_annotation(&self, tab: &str, backend_node_id: u64) -> Result<(), String> {
        annotate::pick(&self.app, tab, backend_node_id);
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.state().annotate.is_frozen(tab) {
            if Instant::now() >= deadline {
                return Err("selection did not freeze".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.annotate_control(tab, "annotateFrame").map(|_| ())
    }

    pub fn place_browser(&self, tab: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(bounds::browser_set_bounds(
            self.state(),
            tab.to_owned(),
            0.0,
            0.0,
            960.0,
            480.0,
        ))
    }

    pub fn cover_browser(&self, tab: &str) -> Result<(), String> {
        tauri::async_runtime::block_on(page_controls::browser_hide(
            self.app.clone(),
            self.state(),
            tab.to_owned(),
            Some(true),
        ))
    }

    pub fn browser_visible(&self, tab: &str) -> Result<bool, String> {
        let webview = self
            .state()
            .webview_for_tab(tab)
            .ok_or("browser tab unavailable")?;
        let (sender, receiver) = mpsc::sync_channel(1);
        webview
            .with_webview(move |platform| {
                let mut visible = Default::default();
                let result = unsafe { platform.controller().IsVisible(&mut visible) }
                    .map(|()| visible.as_bool())
                    .map_err(|error| error.to_string());
                let _ = sender.send(result);
            })
            .map_err(|error| error.to_string())?;
        receiver
            .recv_timeout(Duration::from_secs(5))
            .map_err(|error| error.to_string())?
    }
}
