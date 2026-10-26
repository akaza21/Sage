//! Annotate's pick (M1b): a click in inspect mode names the element, the
//! element is read in the isolated world and the accessibility tree, the
//! page never sees the click, its main world sees nothing, and no form value
//! reaches the item.

use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::Cursor;

use serde_json::{Value, json};

use super::{
    CSRF_CANARY, Lane, PASSWORD_CANARY,
    annotate::{
        assert_main_world_untouched, cdp, mouse, overlay_event, press_button, watch_main_world,
    },
};
use crate::browser::annotate::element;

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

/// A PNG screenshot as rows of pixel bytes.
fn png_rows(data: &Value) -> Vec<Vec<u8>> {
    let bytes = STANDARD.decode(data.as_str().unwrap()).unwrap();
    let mut reader = png::Decoder::new(Cursor::new(bytes)).read_info().unwrap();
    let mut pixels = vec![0; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut pixels).unwrap();
    pixels[..frame.buffer_size()]
        .chunks(frame.line_size)
        .map(<[u8]>::to_vec)
        .collect()
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
    // The isolated world reads the real page, not the main world's lies.
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
        (viewport.client_width * viewport.scale).round()
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

    // Picking paused hovering; resuming brings it back.
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

#[test]
fn the_frame_shows_the_scrolled_viewport() {
    let mut lane = Lane::open("/big.html");
    lane.evaluate("scrollTo(0, 2000); true");
    lane.capability.port_mut().start_inspect().unwrap();
    let backend = backend_of(&mut lane, "li:nth-child(120) a");
    let picked = lane.capability.port_mut().pick_element(backend).unwrap();
    let viewport = picked.page.viewport;
    assert_eq!(viewport.y, 2000.0);
    let top = picked.page.rect.y - viewport.y;
    assert!(top >= 0.0 && top < viewport.client_height, "{top}");
    // The frame clip is in page coordinates: it shows the visible viewport,
    // which a plain screenshot also shows (with the scrollbar beside it).
    let clip = |x: f64, y: f64| json!({"format": "png", "clip": {"x": x, "y": y, "width": viewport.client_width, "height": viewport.client_height, "scale": 1}});
    let shown = png_rows(
        &cdp(
            &mut lane,
            "Page.captureScreenshot",
            json!({"format": "png"}),
        )["data"],
    );
    let framed = png_rows(
        &cdp(
            &mut lane,
            "Page.captureScreenshot",
            clip(viewport.x, viewport.y),
        )["data"],
    );
    let top = png_rows(&cdp(&mut lane, "Page.captureScreenshot", clip(0.0, 0.0))["data"]);
    assert_eq!(framed.len(), shown.len());
    let visible = |rows: &[Vec<u8>]| -> Vec<Vec<u8>> {
        rows.iter()
            .map(|row| row[..framed[0].len()].to_vec())
            .collect()
    };
    assert!(
        visible(&shown) == framed,
        "the frame matches what the page shows"
    );
    assert!(
        visible(&shown) != top,
        "a viewport-relative clip would show the top"
    );
}
