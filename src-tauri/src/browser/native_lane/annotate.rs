//! Production chrome paths and native visibility observations for BP-H2b.

use std::{
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

use serde_json::{Value, json};

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

    /// Sends trusted browser input through the production isolated-world picker,
    /// then waits for the completed selection rather than only its freeze mark.
    pub fn pick_annotation(&self, tab: &str, backend_node_id: u64) -> Result<(), String> {
        let model = self.cdp(
            tab,
            "DOM.getBoxModel",
            &json!({"backendNodeId": backend_node_id}),
        )?;
        let quad = model["model"]["content"]
            .as_array()
            .ok_or("no picked box")?;
        let coordinate = |index: usize| {
            quad.get(index)
                .and_then(Value::as_f64)
                .ok_or("invalid picked box")
        };
        let x = (coordinate(0)? + coordinate(4)?) / 2.0;
        let y = (coordinate(1)? + coordinate(5)?) / 2.0;
        self.drag_annotation(tab, x, y, x, y)
    }

    pub fn drag_annotation(
        &self,
        tab: &str,
        x: f64,
        y: f64,
        end_x: f64,
        end_y: f64,
    ) -> Result<(), String> {
        if self.state().annotate.is_frozen(tab) {
            return Err("a browser selection is already waiting".into());
        }
        for (kind, x, y, buttons) in [
            ("mousePressed", x, y, 1),
            ("mouseMoved", end_x, end_y, 1),
            ("mouseReleased", end_x, end_y, 0),
        ] {
            self.cdp(tab, "Input.dispatchMouseEvent", &json!({
                "type": kind, "x": x, "y": y, "button": "left", "buttons": buttons, "clickCount": 1,
            }))?;
        }
        let deadline = Instant::now() + Duration::from_secs(10);
        while !self.state().annotate.is_frozen(tab) {
            if Instant::now() >= deadline {
                return Err("selection did not freeze".into());
            }
            thread::sleep(Duration::from_millis(10));
        }
        self.annotate_control(tab, "annotateFrame").map(|_| ())
    }

    pub fn zoom_browser(&self, tab: &str, percent: u16) -> Result<(), String> {
        tauri::async_runtime::block_on(page_controls::browser_zoom(
            self.state(),
            tab.to_owned(),
            percent,
        ))
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

    /// Reads WebView2's actual controller visibility on its owning thread.
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
