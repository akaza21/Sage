//! The production device toolbar commands for native regression tests.

use serde_json::Value;
use tauri::{LogicalSize, Manager};

use super::Lane;
use crate::browser::{bounds, device};

impl Lane {
    pub fn show_device_window(&self) -> Result<(), String> {
        let window = self
            .app
            .get_window("main")
            .ok_or("main window unavailable")?;
        window
            .set_size(LogicalSize::new(1000.0, 1000.0))
            .map_err(|error| error.to_string())?;
        window.show().map_err(|error| error.to_string())
    }

    /// The same bounds and device commands the toolbar sends, with no emulation
    /// added by the test driver.
    pub fn set_device(&self, tab: &str, request: Option<Value>) -> Result<(), String> {
        let request = request
            .map(serde_json::from_value::<device::DeviceRequest>)
            .transpose()
            .map_err(|error| error.to_string())?;
        tauri::async_runtime::block_on(device::browser_set_device(
            self.state(),
            tab.to_owned(),
            request,
        ))
        .map_err(|error| format!("{error:?}"))
    }

    pub fn place_device(&self, tab: &str, width: f64, height: f64) -> Result<(), String> {
        tauri::async_runtime::block_on(bounds::browser_set_bounds(
            self.state(),
            tab.to_owned(),
            0.0,
            0.0,
            width,
            height,
        ))
    }
}
