//! The CDP calls behind a device emulation, shared by the WebView2 path and
//! the headless Chromium lane.

use serde_json::{Value, json};

use super::{Agent, DeviceEmulation};

const MAX_TOUCH_POINTS: u8 = 5;
const IOS_VERSION: &str = "18_5";
const IOS_SAFARI: &str = "18.5";

impl DeviceEmulation {
    /// Whether applying it needs the engine's Chrome major version.
    pub(super) fn needs_engine_version(&self) -> bool {
        matches!(self.preset.agent, Agent::Android { .. })
    }

    /// The CDP calls that apply it, in order. `chrome_major` is required
    /// only when [`Self::needs_engine_version`] says so.
    pub(super) fn commands(&self, chrome_major: Option<u32>) -> Vec<(&'static str, Value)> {
        let preset = self.preset;
        let mut metrics = json!({
            "width": self.width,
            "height": self.height,
            "deviceScaleFactor": preset.scale_factor,
            "mobile": preset.mobile,
            "scale": f64::from(self.scale_percent) / 100.0,
            "screenWidth": self.width,
            "screenHeight": self.height,
            "positionX": 0,
            "positionY": 0,
            // The WebView already has the drawn size; only the page's view
            // of it changes, as in DevTools device mode.
            "dontSetVisibleSize": true,
        });
        if preset.mobile {
            let landscape = self.width > self.height;
            metrics["screenOrientation"] = json!({
                "type": if landscape { "landscapePrimary" } else { "portraitPrimary" },
                "angle": if landscape { 90 } else { 0 },
            });
        }
        let touch = if preset.mobile {
            json!({"enabled": true, "maxTouchPoints": MAX_TOUCH_POINTS})
        } else {
            json!({"enabled": false})
        };
        vec![
            ("Emulation.setDeviceMetricsOverride", metrics),
            ("Emulation.setTouchEmulationEnabled", touch),
            (
                "Emulation.setUserAgentOverride",
                user_agent(preset.agent, chrome_major),
            ),
        ]
    }
}

fn user_agent(agent: Agent, chrome_major: Option<u32>) -> Value {
    match agent {
        // An empty user agent drops the override.
        Agent::Engine => json!({"userAgent": ""}),
        Agent::Apple { device } => {
            let os = if device == "iPad" { "OS" } else { "iPhone OS" };
            json!({
                "userAgent": format!(
                    "Mozilla/5.0 ({device}; CPU {os} {IOS_VERSION} like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/{IOS_SAFARI} Mobile/15E148 Safari/604.1"
                ),
                "platform": device,
            })
        }
        Agent::Android { model } => {
            let major = chrome_major.unwrap_or_default().to_string();
            json!({
                "userAgent": format!(
                    "Mozilla/5.0 (Linux; Android 10; K) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{major}.0.0.0 Mobile Safari/537.36"
                ),
                "platform": "Linux armv81",
                "userAgentMetadata": {
                    "brands": [
                        {"brand": "Chromium", "version": major},
                        {"brand": "Not_A Brand", "version": "24"},
                    ],
                    "platform": "Android",
                    "platformVersion": "14.0.0",
                    "architecture": "",
                    "model": model,
                    "mobile": true,
                },
            })
        }
    }
}

/// The CDP calls that return the tab to its own viewport and user agent.
pub(super) fn reset_commands() -> Vec<(&'static str, Value)> {
    vec![
        ("Emulation.clearDeviceMetricsOverride", json!({})),
        (
            "Emulation.setTouchEmulationEnabled",
            json!({"enabled": false}),
        ),
        ("Emulation.setUserAgentOverride", json!({"userAgent": ""})),
    ]
}

/// Reads the major version from `Browser.getVersion`'s `product`, such as
/// `Chrome/131.0.6778.86` or `HeadlessChrome/131.0.6778.86`.
pub(super) fn chrome_major(version: &str) -> Option<u32> {
    let product = serde_json::from_str::<Value>(version).ok()?;
    let (_, number) = product.get("product")?.as_str()?.rsplit_once('/')?;
    let major = number.split('.').next()?.parse().ok()?;
    (1..10_000).contains(&major).then_some(major)
}

/// Applies `emulation`, or resets the tab when it is `None`, through `call`.
/// On a failed step it tries to reset, so a tab is never left half emulated.
pub(crate) fn apply_with(
    emulation: Option<&DeviceEmulation>,
    mut call: impl FnMut(&'static str, &str) -> Result<String, String>,
) -> Result<(), String> {
    let commands = match emulation {
        None => reset_commands(),
        Some(emulation) => {
            let major = if emulation.needs_engine_version() {
                Some(
                    call("Browser.getVersion", "{}")
                        .ok()
                        .as_deref()
                        .and_then(chrome_major)
                        .ok_or("browser engine version unavailable")?,
                )
            } else {
                None
            };
            emulation.commands(major)
        }
    };
    let result = commands
        .iter()
        .try_for_each(|(method, params)| call(method, &params.to_string()).map(drop));
    if result.is_err() && emulation.is_some() {
        for (method, params) in reset_commands() {
            let _ = call(method, &params.to_string());
        }
    }
    result
}
