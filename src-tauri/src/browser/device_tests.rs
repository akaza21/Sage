use serde_json::{Value, json};

use super::{
    DeviceEmulation, DeviceFailure, DeviceHeld, DeviceRequest, Left, PRESETS, RESPONSIVE, admit,
    apply_with,
    emulation::{chrome_major, reset_commands},
    held_after,
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

fn reset_methods() -> Vec<&'static str> {
    reset_commands()
        .into_iter()
        .map(|(method, _)| method)
        .collect()
}

fn engine_reply(method: &str) -> String {
    match method {
        "Browser.getVersion" => r#"{"product":"Chrome/131.0.1.2"}"#,
        "Page.getLayoutMetrics" => {
            r#"{"cssVisualViewport":{"clientWidth":50,"clientHeight":700,"scale":1,"zoom":1}}"#
        }
        _ => "{}",
    }
    .into()
}

#[test]
fn a_failed_step_resets_the_tab_and_confirms_it() {
    let mut sent = Vec::new();
    let mut refused = false;
    let failed = apply_with(Some(&admitted("iphone-se", 375, 667, 100)), |method, _| {
        sent.push(method);
        if method == "Emulation.setTouchEmulationEnabled" && !refused {
            refused = true;
            Err("refused".into())
        } else {
            Ok(engine_reply(method))
        }
    });
    assert_eq!(failed, Err(("refused".into(), Left::Reset)));
    let reset = sent
        .iter()
        .position(|method| *method == "Emulation.clearDeviceMetricsOverride")
        .unwrap();
    assert_eq!(&sent[reset..], reset_methods().as_slice());
}

#[test]
fn an_android_switch_without_an_engine_version_resets_the_emulated_tab() {
    for version in [Err("gone".to_owned()), Ok("{}".to_owned())] {
        let mut sent = Vec::new();
        let result = apply_with(Some(&admitted("pixel-7", 412, 915, 100)), |method, _| {
            sent.push(method);
            if method == "Browser.getVersion" {
                version.clone()
            } else {
                Ok("{}".into())
            }
        });
        assert_eq!(result.map_err(|(_, left)| left), Err(Left::Reset));
        assert_eq!(sent[0], "Browser.getVersion");
        assert_eq!(&sent[1..], reset_methods().as_slice());
    }
}

#[test]
fn a_reset_tries_every_step_and_reports_an_unconfirmed_one() {
    for emulation in [None, Some(admitted("iphone-se", 375, 667, 100))] {
        let mut sent = Vec::new();
        let result = apply_with(emulation.as_ref(), |method, _| {
            sent.push(method);
            if method == "Emulation.clearDeviceMetricsOverride"
                || (emulation.is_some() && method == "Emulation.setDeviceMetricsOverride")
            {
                Err("refused".into())
            } else {
                Ok(engine_reply(method))
            }
        });
        assert_eq!(result, Err(("refused".into(), Left::Unknown)));
        assert_eq!(
            &sent[sent.len() - reset_methods().len()..],
            reset_methods().as_slice()
        );
    }
    let mut sent = Vec::new();
    assert_eq!(
        apply_with(None, |method, _| {
            sent.push(method);
            Ok("{}".into())
        }),
        Ok(())
    );
    assert_eq!(sent, reset_methods());
}

#[test]
fn the_toolbar_is_told_what_the_tab_holds_after_an_update() {
    let pixel = admitted("pixel-7", 412, 915, 100);
    let failed = |left| Err(("refused".to_owned(), left));
    assert_eq!(
        held_after(Some(pixel), Ok(())),
        Ok(DeviceHeld::Known(Some(pixel)))
    );
    assert_eq!(held_after(None, Ok(())), Ok(DeviceHeld::Known(None)));
    assert_eq!(
        held_after(Some(pixel), failed(Left::Reset)),
        Err(DeviceFailure::new("refused", DeviceHeld::Known(None)))
    );
    for requested in [Some(pixel), None] {
        assert_eq!(
            held_after(requested, failed(Left::Unknown)),
            Err(DeviceFailure::new("refused", DeviceHeld::Unconfirmed))
        );
    }
    assert_eq!(
        serde_json::to_value(DeviceFailure::new("refused", DeviceHeld::Known(None))).unwrap(),
        json!({"message": "refused", "held": {"state": "known", "device": null}})
    );
    assert_eq!(
        serde_json::to_value(DeviceFailure::new("refused", DeviceHeld::Unconfirmed)).unwrap(),
        json!({"message": "refused", "held": {"state": "unconfirmed"}})
    );
}

#[test]
fn the_tab_holds_its_emulation_without_changing_the_page_revision() {
    let mut tabs = BrowserTabs::<usize>::new();
    tabs.insert("browser-1".into(), 1, false);
    let before = tabs.snapshot("browser-1").unwrap().revision;
    let device = admitted(RESPONSIVE, 400, 800, 100);
    let request = tabs.request_device("browser-1").unwrap();
    assert!(tabs.settle_device("browser-1", request, DeviceHeld::Known(Some(device))));
    let snapshot = tabs.snapshot("browser-1").unwrap();
    assert_eq!(snapshot.device, DeviceHeld::Known(Some(device)));
    assert_eq!(snapshot.revision, before);
    assert_eq!(
        serde_json::to_value(device).unwrap(),
        json!({"preset": "responsive", "width": 400, "height": 800, "scalePercent": 100})
    );
    let request = tabs.request_device("browser-1").unwrap();
    assert!(tabs.settle_device("browser-1", request, DeviceHeld::Known(None)));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().device,
        DeviceHeld::Known(None)
    );
    let request = tabs.request_device("browser-1").unwrap();
    assert!(tabs.settle_device("browser-1", request, DeviceHeld::Unconfirmed));
    let held = tabs.snapshot("browser-1").unwrap().device;
    assert_eq!(held, DeviceHeld::Unconfirmed);
    assert_eq!((held.confirmed(), held.is_unconfirmed()), (None, true));
    assert_eq!(tabs.device_held("browser-1"), DeviceHeld::Unconfirmed);
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
    assert!(!tabs.settle_device("browser-1", older, DeviceHeld::Known(Some(device))));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().device,
        DeviceHeld::Known(None)
    );
    assert!(tabs.settle_device("browser-1", newer, DeviceHeld::Known(Some(device))));
    assert_eq!(
        tabs.snapshot("browser-1").unwrap().device,
        DeviceHeld::Known(Some(device))
    );
}
