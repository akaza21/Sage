use serde_json::{Value, json};

use super::{Agent, DeviceEmulation};

const MAX_TOUCH_POINTS: u8 = 5;
const IOS_VERSION: &str = "18_5";
const IOS_SAFARI: &str = "18.5";

impl DeviceEmulation {
    pub(super) fn needs_engine_version(&self) -> bool {
        matches!(self.preset.agent, Agent::Android { .. })
    }

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Left {
    Reset,
    /// A reset step failed, so some override may still be on.
    Unknown,
}

/// Any failure resets the tab, trying every step, so the toolbar never shows
/// a state the page lacks.
pub(crate) fn apply_with(
    emulation: Option<&DeviceEmulation>,
    mut call: impl FnMut(&'static str, &str) -> Result<String, String>,
) -> Result<(), (String, Left)> {
    let applied = match emulation {
        None => Ok(()),
        Some(emulation) => apply_steps(emulation, &mut call),
    };
    let failure = match (applied, emulation) {
        (Ok(()), Some(_)) => return Ok(()),
        (Ok(()), None) => None,
        (Err(message), _) => Some(message),
    };
    // Every step runs even after one fails: each clears its own override.
    let mut reset_error = None;
    for (method, params) in reset_commands() {
        if let Err(message) = call(method, &params.to_string()) {
            reset_error.get_or_insert(message);
        }
    }
    match (failure, reset_error) {
        (None, None) => Ok(()),
        (Some(message), None) => Err((message, Left::Reset)),
        (failure, Some(reset)) => Err((failure.unwrap_or(reset), Left::Unknown)),
    }
}

fn apply_steps(
    emulation: &DeviceEmulation,
    call: &mut impl FnMut(&'static str, &str) -> Result<String, String>,
) -> Result<(), String> {
    let major = if emulation.needs_engine_version() {
        let version = call("Browser.getVersion", "{}")?;
        Some(chrome_major(&version).ok_or("browser engine version unavailable")?)
    } else {
        None
    };
    for (method, params) in emulation.commands(major) {
        if method == "Emulation.setDeviceMetricsOverride" && emulation.preset.mobile {
            prepare_mobile_viewport(emulation, &params, call)?;
        }
        call(method, &params.to_string())?;
    }
    Ok(())
}

/// Resize the desktop layout before enabling mobile viewport calculations.
/// Otherwise Chromium can carry the narrow desktop layout's scale into the
/// mobile viewport. Leave an unchanged viewport alone so fit-percent updates
/// preserve the page's own zoom, including authored initial-scale metadata.
fn prepare_mobile_viewport(
    emulation: &DeviceEmulation,
    metrics: &Value,
    call: &mut impl FnMut(&'static str, &str) -> Result<String, String>,
) -> Result<(), String> {
    let current = call("Page.getLayoutMetrics", "{}")?;
    let current: Value =
        serde_json::from_str(&current).map_err(|_| "browser layout unavailable")?;
    let visual = &current["cssVisualViewport"];
    let number = |field: &str| {
        visual[field]
            .as_f64()
            .filter(|value| *value > 0.0)
            .ok_or("browser layout unavailable")
    };
    // CSS viewport pixels -> device-independent pixels. Neither factor is DPR.
    let scale = number("scale")? * visual["zoom"].as_f64().unwrap_or(1.0);
    let same_width = (number("clientWidth")? * scale - f64::from(emulation.width)).abs() < 1.0;
    let same_height = (number("clientHeight")? * scale - f64::from(emulation.height)).abs() < 1.0;
    if same_width && same_height {
        return Ok(());
    }
    let mut desktop = metrics.clone();
    desktop["mobile"] = json!(false);
    desktop["deviceScaleFactor"] = json!(0);
    call("Emulation.setDeviceMetricsOverride", &desktop.to_string())?;
    // This read flushes layout before the mobile metrics are applied.
    call("Page.getLayoutMetrics", "{}").map(drop)
}
