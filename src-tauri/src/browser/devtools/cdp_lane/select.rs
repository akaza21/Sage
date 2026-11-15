use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Cursor;

use serde_json::json;

use super::{
    CSRF_CANARY, Lane, PASSWORD_CANARY,
    annotate::{
        assert_main_world_untouched, cdp, mouse, overlay_event, press_button, watch_main_world,
    },
};
use crate::browser::annotate::{
    element::{self, PageElement},
    selection::Frame,
};

const EMAIL_CANARY: &str = "CANARY-EMAIL-1c90e2";
const NOTES_CANARY: &str = "CANARY-NOTES-6b2f71";

fn backend_of(lane: &mut Lane, selector: &str) -> u64 {
    let root = cdp(lane, "DOM.getDocument", json!({"depth": 0}));
    let node = cdp(
        lane,
        "DOM.querySelector",
        json!({"nodeId": root["root"]["nodeId"], "selector": selector}),
    )["nodeId"]
        .clone();
    cdp(lane, "DOM.describeNode", json!({"nodeId": node}))["node"]["backendNodeId"]
        .as_u64()
        .unwrap_or_else(|| panic!("{selector} has no backend node"))
}

fn jpeg_size(data: &str) -> (u32, u32) {
    let bytes = STANDARD.decode(data).expect("frame base64");
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    decoder.read_info().expect("frame is a JPEG");
    let info = decoder.info().unwrap();
    (u32::from(info.width), u32::from(info.height))
}

#[test]
fn a_click_in_annotate_picks_the_element_without_reaching_the_page() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    lane.capability.port_mut().start_inspect().unwrap();
    lane.capability.port_mut().transport.take_overlay_events();
    let (node, backend, x, y) = press_button(&mut lane);
    let border =
        cdp(&mut lane, "DOM.getBoxModel", json!({"nodeId": node}))["model"]["border"].clone();
    mouse(&mut lane, "mouseMoved", x, y);
    mouse(&mut lane, "mousePressed", x, y);
    mouse(&mut lane, "mouseReleased", x, y);
    let clicked = overlay_event(&mut lane, "Overlay.inspectNodeRequested")
        .expect("a click in inspect mode names the node");
    assert_eq!(clicked["backendNodeId"].as_u64(), Some(backend));

    let picked = lane
        .capability
        .port_mut()
        .pick_element(backend)
        .expect("pick the clicked element");
    assert_eq!(picked.page.selector, "#press");
    assert_eq!(picked.page.positional, "html > body > button");
    assert_eq!(picked.page.text, "Press me");
    assert_eq!(picked.page.title, "Hostile page");
    assert!(!picked.page.sensitive);
    assert_eq!(
        (picked.role.as_str(), picked.name.as_str()),
        ("button", "Press me")
    );
    let corner = |index: usize| border[index].as_f64().unwrap();
    assert!((picked.page.rect.x - corner(0)).abs() < 1.0);
    assert!((picked.page.rect.y - corner(1)).abs() < 1.0);
    assert!((picked.page.rect.width - (corner(4) - corner(0))).abs() < 1.0);
    let viewport = picked.page.viewport;
    assert!(viewport.admitted());
    assert_eq!(
        jpeg_size(&picked.frame.jpeg_base64),
        (picked.frame.width, picked.frame.height)
    );
    assert_eq!(
        f64::from(picked.frame.width),
        (viewport.width * viewport.scale).round()
    );

    let url = lane.site.url("/hostile.html");
    let item = element::pick(&picked.page, &picked.role, &picked.name, "browser-1", &url)
        .and_then(|picked| element::confirm(&picked, 1, "Make this primary"))
        .expect("a valid element item");
    assert_eq!(
        serde_json::to_value(&item).unwrap()["box"],
        json!({
            "x": picked.page.rect.x.round() as i64,
            "y": picked.page.rect.y.round() as i64,
            "width": picked.page.rect.width.round() as i64,
            "height": picked.page.rect.height.round() as i64
        })
    );

    mouse(&mut lane, "mouseMoved", x + 1.0, y);
    assert!(overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none());
    lane.capability.port_mut().start_inspect().unwrap();
    mouse(&mut lane, "mouseMoved", x + 2.0, y);
    assert!(overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_some());

    assert!(
        !lane.wait_text("Pressed for real", 300),
        "the click stayed inert"
    );
    assert_main_world_untouched(&mut lane);
    lane.capability.port_mut().stop_inspect().unwrap();
}

#[test]
fn a_pick_never_reads_form_values_or_hidden_inputs() {
    let mut lane = Lane::open("/form.html");
    for (field, canary) in [("email", EMAIL_CANARY), ("notes", NOTES_CANARY)] {
        lane.evaluate(&format!("document.getElementById('{field}').focus(); true"));
        cdp(&mut lane, "Input.insertText", json!({"text": canary}));
    }
    assert_eq!(
        lane.evaluate("document.getElementById('email').value"),
        json!(EMAIL_CANARY)
    );
    lane.capability.port_mut().start_inspect().unwrap();
    let url = lane.site.url("/form.html");
    let mut items = Vec::new();
    for (selector, sensitive) in [
        ("form", false),
        ("#email", false),
        ("#password", true),
        ("input[type=hidden]", true),
        ("#notes", false),
    ] {
        let backend = backend_of(&mut lane, selector);
        let picked = lane
            .capability
            .port_mut()
            .pick_element(backend)
            .unwrap_or_else(|error| panic!("pick {selector}: {error:?}"));
        assert_eq!(picked.page.sensitive, sensitive, "{selector}");
        let item = element::pick(&picked.page, &picked.role, &picked.name, "browser-1", &url)
            .and_then(|picked| element::confirm(&picked, 1, ""))
            .unwrap_or_else(|error| panic!("item for {selector}: {error}"));
        if sensitive {
            assert_eq!(item.name, "", "{selector} keeps its role only");
        }
        items.push((selector, item));
        lane.capability.port_mut().start_inspect().unwrap();
    }
    let form = &items[0].1;
    assert_eq!(form.text, "Email Password Notes Submit");
    let all =
        serde_json::to_string(&items.iter().map(|(_, item)| item).collect::<Vec<_>>()).unwrap();
    for canary in [PASSWORD_CANARY, CSRF_CANARY, EMAIL_CANARY, NOTES_CANARY] {
        assert!(!all.contains(canary), "{canary} reached an item: {all}");
    }
    lane.capability.port_mut().stop_inspect().unwrap();
}

const SHIP_CANARY: &str = "CANARY-SHIP-2a7c58";
const SELECT_CANARY: &str = "CANARY-SELECT-4e8a13";

fn item_of(lane: &mut Lane, selector: &str) -> (PageElement, Frame, element::BrowserElement) {
    lane.capability.port_mut().start_inspect().unwrap();
    let backend = backend_of(lane, selector);
    let picked = lane.capability.port_mut().pick_element(backend).unwrap();
    let url = lane.site.url("/annotate.html");
    let item = element::pick(&picked.page, &picked.role, &picked.name, "browser-1", &url)
        .and_then(|picked| element::confirm(&picked, 1, ""))
        .unwrap_or_else(|error| panic!("item for {selector}: {error}"));
    (picked.page, picked.frame, item)
}

#[test]
fn a_name_that_may_embed_a_form_value_never_reaches_the_item() {
    let mut lane = Lane::open("/annotate.html");
    lane.evaluate("document.getElementById('ship').focus(); true");
    cdp(&mut lane, "Input.insertText", json!({"text": SHIP_CANARY}));
    lane.evaluate("document.getElementById('speed').selectedIndex = 1; true");
    for selector in ["#wrapped-input", "#wrapped-select", "#ship"] {
        let (page, _, item) = item_of(&mut lane, selector);
        // A wrapper's name is built from its content, values included; the
        // input's own name comes from its label alone.
        let wraps = selector != "#ship";
        assert_eq!(page.controls, wraps, "{selector}");
        assert_eq!(
            item.name.trim(),
            if wraps { "" } else { "Ship to" },
            "{selector}"
        );
        let all = serde_json::to_string(&item).unwrap();
        for canary in [SHIP_CANARY, SELECT_CANARY] {
            assert!(!all.contains(canary), "{canary} reached {selector}: {all}");
        }
    }
    let (page, _, item) = item_of(&mut lane, "#press");
    assert!(!page.controls);
    assert_eq!(item.name, "Press me");
}

#[test]
fn text_the_user_cannot_see_never_reaches_the_item() {
    let mut lane = Lane::open("/annotate.html");
    let (_, _, item) = item_of(&mut lane, "#hidden-text");
    assert_eq!(item.text, "Visible words");
}

/// In image pixels: left, top, right, bottom.
fn red_box(frame: &Frame) -> (f64, f64, f64, f64) {
    let bytes = STANDARD.decode(&frame.jpeg_base64).expect("frame base64");
    let mut decoder = jpeg_decoder::Decoder::new(Cursor::new(bytes));
    let pixels = decoder.decode().expect("frame is a JPEG");
    let width = usize::from(decoder.info().unwrap().width);
    let (mut left, mut top, mut right, mut bottom) = (usize::MAX, usize::MAX, 0, 0);
    for (index, pixel) in pixels.chunks(3).enumerate() {
        if pixel[0] > 200 && pixel[1] < 80 && pixel[2] < 80 {
            let (x, y) = (index % width, index / width);
            (left, top) = (left.min(x), top.min(y));
            (right, bottom) = (right.max(x + 1), bottom.max(y + 1));
        }
    }
    assert!(right > left, "the block is in the frame");
    (left as f64, top as f64, right as f64, bottom as f64)
}

#[test]
fn the_outline_lands_on_the_element_in_the_frozen_frame() {
    let mut lane = Lane::open("/annotate.html");
    let (plain, _, _) = item_of(&mut lane, "#block");
    for (setup, undo, device_scale) in [
        ("", "", 1.0),
        ("scrollTo(0, 10); true", "scrollTo(0, 0); true", 1.0),
        (
            "Emulation.setDeviceMetricsOverride",
            "Emulation.clearDeviceMetricsOverride",
            1.5,
        ),
        (
            "Emulation.setPageScaleFactor",
            "Emulation.setPageScaleFactor",
            2.0,
        ),
    ] {
        match setup {
            "" => {}
            "Emulation.setDeviceMetricsOverride" => {
                let (width, height) = (plain.viewport.width, plain.viewport.height);
                cdp(
                    &mut lane,
                    setup,
                    json!({"width": width, "height": height, "deviceScaleFactor": 1.5, "mobile": false}),
                );
            }
            "Emulation.setPageScaleFactor" => {
                cdp(&mut lane, setup, json!({"pageScaleFactor": 2}));
            }
            script => {
                lane.evaluate(script);
            }
        }
        let (page, frame, _) = item_of(&mut lane, "#block");
        let viewport = page.viewport;
        let scale = f64::from(frame.width) / viewport.width;
        assert_eq!(viewport.scale, device_scale, "{setup}");
        assert!((scale - device_scale).abs() < 0.01, "{setup}: {scale}");
        assert!(
            (f64::from(frame.height) / viewport.height - scale).abs() < 0.01,
            "{setup}: the frame keeps the page's proportions"
        );
        let (left, top, right, bottom) = red_box(&frame);
        let expected = [
            page.rect.x - viewport.x,
            page.rect.y - viewport.y,
            page.rect.x - viewport.x + page.rect.width,
            page.rect.y - viewport.y + page.rect.height,
        ];
        let found = [left, top, right, bottom].map(|side| side / scale);
        for (found, expected) in found.iter().zip(expected) {
            assert!(
                (found - expected).abs() <= 1.0,
                "{setup}: outline {expected:?} vs picture {found:?}"
            );
        }
        match undo {
            "" => {}
            "Emulation.clearDeviceMetricsOverride" => {
                cdp(&mut lane, undo, json!({}));
            }
            "Emulation.setPageScaleFactor" => {
                cdp(&mut lane, undo, json!({"pageScaleFactor": 1}));
            }
            script => {
                lane.evaluate(script);
            }
        }
    }
}
