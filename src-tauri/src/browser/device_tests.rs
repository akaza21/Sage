use serde_json::{Value, json};

use super::{
    DeviceEmulation, DeviceRequest, PRESETS, RESPONSIVE, admit, apply_with,
    emulation::{chrome_major, reset_commands},
};
use crate::browser::BrowserTabs;

fn request(preset: &str, width: u32, height: u32, scale_percent: u16) -> DeviceRequest {
    DeviceRequest {
        preset: preset.into(),
        width,
        height,
        scale_percent,
    }
}

fn admitted(preset: &str, width: u32, height: u32, scale_percent: u16) -> DeviceEmulation {
    admit(&request(preset, width, height, scale_percent)).expect("admitted device")
}

fn params<'a>(commands: &'a [(&'static str, Value)], method: &str) -> &'a Value {
    &commands
        .iter()
        .find(|(name, _)| *name == method)
        .unwrap_or_else(|| panic!("{method} missing"))
        .1
}

#[test]
fn the_toolbar_preset_list_mirrors_the_rust_table() {
    let mirror: Value = serde_json::from_str(include_str!(
        "../../../src/features/browser/browserDevicePresets.json"
    ))
    .expect("preset JSON");
    let expected: Vec<Value> = PRESETS
        .iter()
        .map(|preset| {
            json!({
                "id": preset.id,
                "label": preset.label,
                "width": preset.width,
                "height": preset.height,
            })
        })
        .collect();
    assert_eq!(mirror, Value::Array(expected));
}

#[test]
fn responsive_sizes_and_scales_are_bounded() {
    assert!(admit(&request(RESPONSIVE, 50, 3840, 10)).is_ok());
    assert!(admit(&request(RESPONSIVE, 922, 992, 100)).is_ok());
    for (width, height, scale) in [
        (49, 500, 100),
        (500, 49, 100),
        (3841, 500, 100),
        (500, u32::MAX, 100),
        (500, 500, 9),
        (500, 500, 101),
        (500, 500, 0),
    ] {
        assert!(
            admit(&request(RESPONSIVE, width, height, scale)).is_err(),
            "{width}x{height} at {scale}%"
        );
    }
}

#[test]
fn fixed_presets_admit_only_their_own_size_in_either_orientation() {
    assert!(admit(&request("iphone-se", 375, 667, 100)).is_ok());
    assert!(admit(&request("iphone-se", 667, 375, 100)).is_ok());
    assert!(admit(&request("iphone-se", 376, 667, 100)).is_err());
    assert!(admit(&request("iphone-se", 412, 915, 100)).is_err());
    assert!(
        admit(&request(
            "Emulation.setDeviceMetricsOverride",
            375,
            667,
            100
        ))
        .is_err()
    );
    assert!(admit(&request("", 375, 667, 100)).is_err());
}

#[test]
fn requests_reject_unknown_fields() {
    let raw =
        r#"{"preset":"responsive","width":400,"height":400,"scalePercent":100,"userAgent":"x"}"#;
    assert!(serde_json::from_str::<DeviceRequest>(raw).is_err());
}

#[test]
fn responsive_overrides_only_the_viewport_and_drops_device_overrides() {
    let device = admitted(RESPONSIVE, 922, 992, 80);
    let commands = device.commands(None);
    let metrics = params(&commands, "Emulation.setDeviceMetricsOverride");
    assert_eq!(metrics["width"], 922);
    assert_eq!(metrics["height"], 992);
    assert_eq!(metrics["deviceScaleFactor"], 0.0);
    assert_eq!(metrics["mobile"], false);
    assert_eq!(metrics["scale"], 0.8);
    assert_eq!(metrics["dontSetVisibleSize"], true);
    assert!(metrics.get("screenOrientation").is_none());
    assert_eq!(
        params(&commands, "Emulation.setTouchEmulationEnabled"),
        &json!({"enabled": false})
    );
    assert_eq!(
        params(&commands, "Emulation.setUserAgentOverride"),
        &json!({"userAgent": ""})
    );
}

#[test]
fn phone_presets_emulate_touch_scale_orientation_and_user_agent() {
    let portrait = admitted("iphone-se", 375, 667, 100).commands(None);
    let metrics = params(&portrait, "Emulation.setDeviceMetricsOverride");
    assert_eq!(metrics["deviceScaleFactor"], 2.0);
    assert_eq!(metrics["mobile"], true);
    assert_eq!(metrics["screenOrientation"]["type"], "portraitPrimary");
    assert_eq!(
        params(&portrait, "Emulation.setTouchEmulationEnabled"),
        &json!({"enabled": true, "maxTouchPoints": 5})
    );
    let agent = params(&portrait, "Emulation.setUserAgentOverride");
    assert!(agent["userAgent"].as_str().unwrap().contains("iPhone OS"));
    assert_eq!(agent["platform"], "iPhone");
    assert!(agent.get("userAgentMetadata").is_none());

    let landscape = admitted("iphone-se", 667, 375, 100).commands(None);
    let metrics = params(&landscape, "Emulation.setDeviceMetricsOverride");
    assert_eq!(metrics["screenOrientation"]["type"], "landscapePrimary");
    assert_eq!(metrics["screenOrientation"]["angle"], 90);

    let ipad = admitted("ipad-air", 820, 1180, 100).commands(None);
    let agent = params(&ipad, "Emulation.setUserAgentOverride");
    assert!(
        agent["userAgent"]
            .as_str()
            .unwrap()
            .contains("(iPad; CPU OS ")
    );
}

#[test]
fn android_presets_carry_the_engine_major_in_agent_and_client_hints() {
    let device = admitted("pixel-7", 412, 915, 100);
    assert!(device.needs_engine_version());
    let commands = device.commands(Some(131));
    let agent = params(&commands, "Emulation.setUserAgentOverride");
    assert!(
        agent["userAgent"]
            .as_str()
            .unwrap()
            .contains("Chrome/131.0.0.0 Mobile")
    );
    let hints = &agent["userAgentMetadata"];
    assert_eq!(hints["platform"], "Android");
    assert_eq!(hints["model"], "Pixel 7");
    assert_eq!(hints["mobile"], true);
    assert_eq!(hints["brands"][0]["version"], "131");
}

#[test]
fn engine_major_comes_from_the_product_string() {
    assert_eq!(
        chrome_major(r#"{"product":"Chrome/131.0.6778.86"}"#),
        Some(131)
    );
    assert_eq!(
        chrome_major(r#"{"product":"HeadlessChrome/140.0.7339.16"}"#),
        Some(140)
    );
    for invalid in [
        "",
        "{}",
        r#"{"product":"Chrome"}"#,
        r#"{"product":"Chrome/x.1"}"#,
        r#"{"product":"Chrome/0.1"}"#,
    ] {
        assert_eq!(chrome_major(invalid), None, "{invalid}");
    }
}

#[test]
fn apply_sends_each_step_in_order_and_resets_after_a_failed_step() {
    let device = admitted("pixel-7", 915, 412, 50);
    let mut sent = Vec::new();
    apply_with(Some(&device), |method, _| {
        sent.push(method);
        Ok(r#"{"product":"Chrome/131.0.1.2"}"#.into())
    })
    .unwrap();
    assert_eq!(
        sent,
        [
            "Browser.getVersion",
            "Emulation.setDeviceMetricsOverride",
            "Emulation.setTouchEmulationEnabled",
            "Emulation.setUserAgentOverride",
        ]
    );

    let mut sent = Vec::new();
    let failed = apply_with(Some(&admitted("iphone-se", 375, 667, 100)), |method, _| {
        sent.push(method);
        if method == "Emulation.setTouchEmulationEnabled" && sent.len() == 2 {
            Err("refused".into())
        } else {
            Ok("{}".into())
        }
    });
    assert_eq!(failed, Err("refused".into()));
    let reset: Vec<_> = reset_commands()
        .into_iter()
        .map(|(method, _)| method)
        .collect();
    assert_eq!(&sent[2..], reset.as_slice());
}

#[test]
fn android_apply_fails_closed_without_an_engine_version() {
    let mut sent = Vec::new();
    let result = apply_with(Some(&admitted("pixel-7", 412, 915, 100)), |method, _| {
        sent.push(method);
        Ok("{}".into())
    });
    assert!(result.is_err());
    assert_eq!(sent, ["Browser.getVersion"]);
}

#[test]
fn the_tab_holds_its_emulation_without_changing_the_page_revision() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let before = tabs.snapshot("browser-1").unwrap().revision;
    let device = admitted(RESPONSIVE, 400, 800, 100);
    let request = tabs.request_device("browser-1").unwrap();
    assert!(tabs.settle_device("browser-1", request, Some(device)));
    let snapshot = tabs.snapshot("browser-1").unwrap();
    assert_eq!(snapshot.device, Some(device));
    assert_eq!(snapshot.revision, before);
    assert_eq!(
        serde_json::to_value(device).unwrap(),
        json!({"preset": "responsive", "width": 400, "height": 800, "scalePercent": 100})
    );
    let request = tabs.request_device("browser-1").unwrap();
    assert!(tabs.settle_device("browser-1", request, None));
    assert_eq!(tabs.snapshot("browser-1").unwrap().device, None);
    assert_eq!(tabs.request_device("browser-2"), None);
}

#[test]
fn only_the_latest_overlapping_update_is_recorded() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let older = tabs.request_device("browser-1").unwrap();
    let newer = tabs.request_device("browser-1").unwrap();
    assert!(!tabs.latest_device("browser-1", older));
    assert!(tabs.latest_device("browser-1", newer));
    let device = admitted("iphone-se", 375, 667, 100);
    assert!(!tabs.settle_device("browser-1", older, Some(device)));
    assert_eq!(tabs.snapshot("browser-1").unwrap().device, None);
    assert!(tabs.settle_device("browser-1", newer, Some(device)));
    assert_eq!(tabs.snapshot("browser-1").unwrap().device, Some(device));
}
