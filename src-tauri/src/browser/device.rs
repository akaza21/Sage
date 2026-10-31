//! The user's device toolbar (BF-7a): a tab-scoped viewport and device
//! emulation applied through CDP `Emulation.*`, after Codex reference 41.
//!
//! The WebView sends a preset id, the CSS viewport size and the scale it
//! drew the page at; Rust admits them against its own preset table and
//! builds every CDP call itself. Neither the page nor the Agent can reach
//! this path, and it never grants a site permission.

use serde::{Deserialize, Serialize};
use tauri::State;

use super::BrowserState;

#[cfg(any(windows, test))]
mod emulation;
#[cfg(any(windows, test))]
pub(crate) use emulation::{Left, apply_with};
#[cfg(windows)]
mod native;

/// The free-size preset; every other preset has a fixed size.
pub(crate) const RESPONSIVE: &str = "responsive";
/// Viewport bounds in CSS px, for Responsive and for either orientation.
const MIN_SIDE: u32 = 50;
const MAX_SIDE: u32 = 3840;
/// The share of its CSS size the page is drawn at so it fits the pane.
const MIN_SCALE_PERCENT: u16 = 10;
const MAX_SCALE_PERCENT: u16 = 100;

#[cfg(not(windows))]
const UNAVAILABLE_REASON: Option<&str> = Some("Device toolbar is unavailable on this platform");
#[cfg(windows)]
const UNAVAILABLE_REASON: Option<&str> = None;

#[derive(Clone, Copy, Debug, PartialEq)]
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
enum Agent {
    /// Keeps the engine's own user agent.
    Engine,
    /// Mobile Safari on an iPhone or iPad, as Safari reports it.
    Apple { device: &'static str },
    /// Chrome on Android, with matching client hints.
    Android { model: &'static str },
}

#[derive(Debug, PartialEq)]
#[cfg_attr(not(any(windows, test)), allow(dead_code))]
pub(crate) struct DevicePreset {
    pub(crate) id: &'static str,
    pub(crate) label: &'static str,
    pub(crate) width: u32,
    pub(crate) height: u32,
    /// Zero keeps the display's own scale factor.
    scale_factor: f64,
    mobile: bool,
    agent: Agent,
}

/// Sage's device list: Chrome DevTools' default phones and tablets. `src/features/browser/browserDevicePresets.json` mirrors the ids,
/// labels and sizes for the toolbar, and a test keeps the two equal.
pub(crate) const PRESETS: &[DevicePreset] = &[
    DevicePreset {
        id: RESPONSIVE,
        label: "Responsive",
        width: 0,
        height: 0,
        scale_factor: 0.0,
        mobile: false,
        agent: Agent::Engine,
    },
    DevicePreset {
        id: "iphone-se",
        label: "iPhone SE",
        width: 375,
        height: 667,
        scale_factor: 2.0,
        mobile: true,
        agent: Agent::Apple { device: "iPhone" },
    },
    DevicePreset {
        id: "iphone-14-pro-max",
        label: "iPhone 14 Pro Max",
        width: 430,
        height: 932,
        scale_factor: 3.0,
        mobile: true,
        agent: Agent::Apple { device: "iPhone" },
    },
    DevicePreset {
        id: "pixel-7",
        label: "Pixel 7",
        width: 412,
        height: 915,
        scale_factor: 2.625,
        mobile: true,
        agent: Agent::Android { model: "Pixel 7" },
    },
    DevicePreset {
        id: "galaxy-s20-ultra",
        label: "Samsung Galaxy S20 Ultra",
        width: 412,
        height: 915,
        scale_factor: 3.5,
        mobile: true,
        agent: Agent::Android { model: "SM-G988B" },
    },
    DevicePreset {
        id: "ipad-mini",
        label: "iPad Mini",
        width: 768,
        height: 1024,
        scale_factor: 2.0,
        mobile: true,
        agent: Agent::Apple { device: "iPad" },
    },
    DevicePreset {
        id: "ipad-air",
        label: "iPad Air",
        width: 820,
        height: 1180,
        scale_factor: 2.0,
        mobile: true,
        agent: Agent::Apple { device: "iPad" },
    },
];

/// What the toolbar asks for.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct DeviceRequest {
    preset: String,
    width: u32,
    height: u32,
    scale_percent: u16,
}

/// An admitted emulation, as the tab holds it and the toolbar restores it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct DeviceEmulation {
    #[serde(serialize_with = "preset_id")]
    preset: &'static DevicePreset,
    width: u32,
    height: u32,
    scale_percent: u16,
}

fn preset_id<S: serde::Serializer>(preset: &&DevicePreset, out: S) -> Result<S::Ok, S::Error> {
    out.serialize_str(preset.id)
}

pub(crate) fn admit(request: &DeviceRequest) -> Result<DeviceEmulation, String> {
    let preset = PRESETS
        .iter()
        .find(|preset| preset.id == request.preset)
        .ok_or("unknown device preset")?;
    let (width, height) = (request.width, request.height);
    let fits = if preset.id == RESPONSIVE {
        [width, height]
            .iter()
            .all(|side| (MIN_SIDE..=MAX_SIDE).contains(side))
    } else {
        (width, height) == (preset.width, preset.height)
            || (width, height) == (preset.height, preset.width)
    };
    if !fits {
        return Err(format!(
            "device size must be {MIN_SIDE}–{MAX_SIDE} px, or the preset's size in either orientation"
        ));
    }
    if !(MIN_SCALE_PERCENT..=MAX_SCALE_PERCENT).contains(&request.scale_percent) {
        return Err("device scale must be 10–100%".into());
    }
    Ok(DeviceEmulation {
        preset,
        width,
        height,
        scale_percent: request.scale_percent,
    })
}

pub(crate) fn unavailable_reason() -> Option<&'static str> {
    UNAVAILABLE_REASON
}

/// What a tab's page holds, as far as Rust can confirm it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(tag = "state", content = "device", rename_all = "camelCase")]
pub(crate) enum DeviceHeld {
    /// The page holds this emulation, or none.
    Known(Option<DeviceEmulation>),
    /// A reset step failed, so some override may still be on.
    Unconfirmed,
}

impl Default for DeviceHeld {
    fn default() -> Self {
        Self::Known(None)
    }
}

/// A tab's device emulation: what the page holds, and how many updates were
/// asked for, so only the latest request lands when two overlap.
#[derive(Default)]
pub(crate) struct TabDevice {
    held: DeviceHeld,
    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    requests: u64,
}

impl TabDevice {
    /// The emulation a remounted toolbar restores: only a confirmed one.
    pub(crate) fn applied(&self) -> Option<DeviceEmulation> {
        match self.held {
            DeviceHeld::Known(device) => device,
            DeviceHeld::Unconfirmed => None,
        }
    }
}

impl<T> super::BrowserTabs<T> {
    /// What the tab's page holds; a missing tab holds nothing.
    pub(super) fn device_held(&self, label: &str) -> DeviceHeld {
        self.entries
            .get(label)
            .map_or_else(DeviceHeld::default, |tab| tab.device.held)
    }
}

#[cfg(any(windows, test))]
impl<T> super::BrowserTabs<T> {
    /// Numbers a new update for the tab; `None` when the tab is gone.
    pub(super) fn request_device(&mut self, label: &str) -> Option<u64> {
        let device = &mut self.entries.get_mut(label)?.device;
        device.requests = device.requests.wrapping_add(1);
        Some(device.requests)
    }

    /// Whether `request` is still the tab's latest update.
    pub(super) fn latest_device(&self, label: &str, request: u64) -> bool {
        self.entries
            .get(label)
            .is_some_and(|tab| tab.device.requests == request)
    }

    /// Records what `request` left on the page, unless a later update owns the
    /// tab now. Returns whether it was recorded.
    pub(super) fn settle_device(&mut self, label: &str, request: u64, held: DeviceHeld) -> bool {
        match self.entries.get_mut(label) {
            Some(tab) if tab.device.requests == request => {
                tab.device.held = held;
                true
            }
            _ => false,
        }
    }
}

/// Why an update failed, with what the tab holds after it, so the toolbar
/// shows what the page has rather than what was asked for.
#[derive(Debug, PartialEq, Serialize)]
pub(crate) struct DeviceFailure {
    message: String,
    held: DeviceHeld,
}

impl DeviceFailure {
    fn new(message: impl Into<String>, held: DeviceHeld) -> Self {
        Self {
            message: message.into(),
            held,
        }
    }

    #[cfg_attr(not(any(windows, test)), allow(dead_code))]
    pub(crate) fn held(&self) -> DeviceHeld {
        self.held
    }
}

#[tauri::command]
pub(crate) async fn browser_set_device(
    state: State<'_, BrowserState>,
    tab_id: String,
    device: Option<DeviceRequest>,
) -> Result<(), DeviceFailure> {
    let device = match device.as_ref().map(admit).transpose() {
        Ok(device) => device,
        Err(message) => {
            // Nothing was sent: the tab keeps what it had.
            let held = state
                .tabs
                .lock()
                .map_or(DeviceHeld::Unconfirmed, |tabs| tabs.device_held(&tab_id));
            return Err(DeviceFailure::new(message, held));
        }
    };
    #[cfg(windows)]
    {
        native::set(&state, tab_id, device).await
    }
    #[cfg(not(windows))]
    {
        let _ = (tab_id, device);
        Err(DeviceFailure::new(
            UNAVAILABLE_REASON.unwrap_or_default(),
            DeviceHeld::default(),
        ))
    }
}

/// What the tab holds after an update: the request when it applied,
/// nothing after a confirmed reset, and an unconfirmed state when a reset
/// step failed, never a guess at which overrides are still on.
#[cfg(any(windows, test))]
pub(crate) fn held_after(
    requested: Option<DeviceEmulation>,
    applied: Result<(), (String, Left)>,
) -> Result<DeviceHeld, DeviceFailure> {
    match applied {
        Ok(()) => Ok(DeviceHeld::Known(requested)),
        Err((message, Left::Reset)) => Err(DeviceFailure::new(message, DeviceHeld::Known(None))),
        Err((message, Left::Unknown)) => Err(DeviceFailure::new(message, DeviceHeld::Unconfirmed)),
    }
}

#[cfg(test)]
#[path = "device_tests.rs"]
mod tests;
