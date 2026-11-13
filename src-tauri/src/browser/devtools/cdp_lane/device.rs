//! The device toolbar (BF-7a): the exact CDP calls `browser::device` builds
//! reach the page as the emulated viewport, scale factor, touch, orientation
//! and user agent, survive a navigation, and reset to the engine's own values.

use std::time::Duration;

use serde_json::{Value, json};

use super::{CdpTransport, Lane};
use crate::browser::device::{DeviceRequest, admit, apply_with};

const PROBE: &str = "({ width: innerWidth, height: innerHeight, screenWidth: screen.width, screenHeight: screen.height, scale: devicePixelRatio, touch: navigator.maxTouchPoints, coarse: matchMedia('(pointer: coarse)').matches, orientation: screen.orientation.type, agent: navigator.userAgent, platform: navigator.platform, mobileHints: navigator.userAgentData ? navigator.userAgentData.mobile : null })";

fn apply(lane: &mut Lane, request: Option<Value>) {
    let device = request.map(|raw| {
        let request: DeviceRequest = serde_json::from_value(raw).expect("device request");
        admit(&request).expect("admitted device")
    });
    let transport = &lane.capability.port_mut().transport;
    apply_with(device.as_ref(), |method, params| {
        transport
            .observe(method, params, 4096, Duration::from_secs(5))
            .map_err(|error| format!("{method}: {error:?}"))
    })
    .expect("device applied");
}

#[test]
fn device_presets_reach_the_page_survive_navigation_and_reset() {
    let mut lane = Lane::open("/landing.html");
    let engine = lane.evaluate(PROBE);
    assert_eq!(engine["touch"], json!(0));

    apply(
        &mut lane,
        Some(json!({"preset": "responsive", "width": 500, "height": 700, "scalePercent": 100})),
    );
    let responsive = lane.evaluate(PROBE);
    assert_eq!(responsive["width"], json!(500));
    assert_eq!(responsive["height"], json!(700));
    assert_eq!(responsive["scale"], engine["scale"]);
    assert_eq!(responsive["agent"], engine["agent"]);
    assert_eq!(responsive["touch"], json!(0));

    // Drawn at half size, the page still lays out at the emulated size.
    apply(
        &mut lane,
        Some(json!({"preset": "responsive", "width": 500, "height": 700, "scalePercent": 50})),
    );
    let half = lane.evaluate(PROBE);
    assert_eq!(
        (half["width"].clone(), half["height"].clone()),
        (json!(500), json!(700))
    );

    apply(
        &mut lane,
        Some(json!({"preset": "pixel-7", "width": 915, "height": 412, "scalePercent": 50})),
    );
    let pixel = lane.evaluate(PROBE);
    assert_eq!(pixel["screenWidth"], json!(915));
    assert_eq!(pixel["screenHeight"], json!(412));
    assert_eq!(pixel["scale"], json!(2.625));
    assert_eq!(pixel["touch"], json!(5));
    assert_eq!(pixel["coarse"], json!(true));
    assert_eq!(pixel["orientation"], json!("landscape-primary"));
    assert_eq!(pixel["platform"], json!("Linux armv81"));
    assert_eq!(pixel["mobileHints"], json!(true));
    let agent = pixel["agent"].as_str().unwrap();
    assert!(
        agent.contains("Android") && agent.contains("Mobile Safari"),
        "{agent}"
    );

    lane.navigate("/navigates.html");
    let navigated = lane.evaluate(PROBE);
    assert_eq!(navigated["screenWidth"], json!(915));
    assert_eq!(navigated["agent"], pixel["agent"]);

    apply(
        &mut lane,
        Some(json!({"preset": "iphone-se", "width": 375, "height": 667, "scalePercent": 100})),
    );
    let iphone = lane.evaluate(PROBE);
    // Without a viewport meta tag a phone lays the page out about 980 px
    // wide, far taller than the screenshot limit at full scale.
    assert!(iphone["width"].as_u64().unwrap() >= 980, "{iphone}");
    assert_eq!(iphone["screenWidth"], json!(375));
    assert_eq!(iphone["scale"], json!(2));
    assert_eq!(iphone["platform"], json!("iPhone"));
    assert!(iphone["agent"].as_str().unwrap().contains("iPhone OS"));

    // The user's screenshot of that tall layout scales down to fit.
    let port = lane.capability.port_mut();
    let revision = port.stamp().expect("stamp").generation.to_string();
    let (shot_width, shot_height, _) = port.capture_user_png(&revision).expect("screenshot");
    assert!(
        shot_width > 0 && shot_height > shot_width,
        "{shot_width}x{shot_height}"
    );

    apply(&mut lane, None);
    let reset = lane.evaluate(PROBE);
    for key in [
        "width", "height", "scale", "touch", "coarse", "agent", "platform",
    ] {
        assert_eq!(reset[key], engine[key], "{key}");
    }
}
