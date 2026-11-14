use std::time::{Duration, Instant};

use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::{Value, json};

use super::{CdpTransport, Lane};

const TIMEOUT: Duration = Duration::from_secs(5);
const BLUE: [u8; 3] = [58, 131, 247];

pub(super) fn cdp(lane: &mut Lane, method: &str, params: Value) -> Value {
    let result = lane
        .capability
        .port_mut()
        .transport
        .observe(method, &params.to_string(), 4 * 1_048_576, TIMEOUT)
        .unwrap_or_else(|error| panic!("{method}: {error:?}"));
    serde_json::from_str(&result).expect("CDP result")
}

pub(super) fn watch_main_world(lane: &mut Lane) {
    lane.evaluate(
        "window.__mutations = 0; new MutationObserver((records) => { window.__mutations += records.length; }).observe(document, { subtree: true, childList: true, attributes: true, characterData: true }); window.__globalsAtLoad = Object.getOwnPropertyNames(window).length; true",
    );
}

pub(super) fn assert_main_world_untouched(lane: &mut Lane) {
    assert_eq!(lane.evaluate("window.__mutations"), json!(0));
    assert_eq!(
        lane.evaluate("Object.getOwnPropertyNames(window).length - window.__globalsAtLoad"),
        json!(0)
    );
    assert_eq!(lane.evaluate("window.__agentSeen"), json!([]));
}

pub(super) fn press_button(lane: &mut Lane) -> (u64, u64, f64, f64) {
    let root = cdp(lane, "DOM.getDocument", json!({"depth": 0}));
    let node = cdp(
        lane,
        "DOM.querySelector",
        json!({"nodeId": root["root"]["nodeId"], "selector": "#press"}),
    )["nodeId"]
        .as_u64()
        .expect("button node");
    let described = cdp(lane, "DOM.describeNode", json!({"nodeId": node}));
    let backend = described["node"]["backendNodeId"].as_u64().unwrap();
    let quad = cdp(lane, "DOM.getBoxModel", json!({"nodeId": node}))["model"]["border"].clone();
    let corner = |index: usize| quad[index].as_f64().unwrap();
    (
        node,
        backend,
        (corner(0) + corner(4)) / 2.0,
        (corner(1) + corner(5)) / 2.0,
    )
}

pub(super) fn mouse(lane: &mut Lane, kind: &str, x: f64, y: f64) {
    let mut params = json!({"type": kind, "x": x, "y": y});
    if kind != "mouseMoved" {
        params["button"] = "left".into();
        params["clickCount"] = 1.into();
    }
    cdp(lane, "Input.dispatchMouseEvent", params);
}

/// A clipped capture renders a fresh frame without the inspect overlay, so
/// the whole viewport is compared.
fn capture(lane: &mut Lane) -> String {
    cdp(lane, "Page.captureScreenshot", json!({"format": "png"}))["data"]
        .as_str()
        .unwrap()
        .to_owned()
}

struct Pixels {
    width: usize,
    rgb: Vec<[u8; 3]>,
}

impl Pixels {
    fn of(capture: &str) -> Self {
        let bytes = STANDARD.decode(capture).expect("capture base64");
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
        let mut reader = decoder.read_info().expect("capture is a PNG");
        let mut buffer = vec![0; reader.output_buffer_size()];
        let frame = reader.next_frame(&mut buffer).expect("capture frame");
        let stride = match frame.color_type {
            png::ColorType::Rgb => 3,
            png::ColorType::Rgba => 4,
            other => panic!("unexpected capture colour type {other:?}"),
        };
        let rgb = buffer[..frame.buffer_size()]
            .chunks_exact(stride)
            .map(|pixel| [pixel[0], pixel[1], pixel[2]])
            .collect();
        Self {
            width: frame.width as usize,
            rgb,
        }
    }

    fn at(&self, x: f64, y: f64) -> [u8; 3] {
        self.rgb[y.floor() as usize * self.width + x.floor() as usize]
    }

    fn is_blue(&self, x: f64, y: f64) -> bool {
        let pixel = self.at(x, y);
        pixel
            .iter()
            .zip(BLUE)
            .all(|(channel, blue)| channel.abs_diff(blue) <= 24)
    }
}

fn capture_scale(lane: &mut Lane, pixels: &Pixels) -> f64 {
    let metrics = cdp(lane, "Page.getLayoutMetrics", json!({}));
    let width = metrics["cssVisualViewport"]["clientWidth"]
        .as_f64()
        .expect("viewport width");
    pixels.width as f64 / width
}

/// `border` is in CSS px: left, top, right, bottom.
fn assert_outlined(hovered: &Pixels, plain: &Pixels, scale: f64, border: [f64; 4]) {
    let [left, top, right, bottom] = border.map(|edge| edge * scale);
    let (middle_x, middle_y) = ((left + right) / 2.0, (top + bottom) / 2.0);
    let edges = [
        ("left", (left, middle_y), (1.0, 0.0)),
        ("right", (right, middle_y), (1.0, 0.0)),
        ("top", (middle_x, top), (0.0, 1.0)),
        ("bottom", (middle_x, bottom), (0.0, 1.0)),
    ];
    for (name, (x, y), (dx, dy)) in edges {
        let blue = (-6..=6)
            .map(f64::from)
            .filter(|step| hovered.is_blue(x + step * dx, y + step * dy))
            .collect::<Vec<_>>();
        assert!(
            blue.iter().any(|step| step.abs() <= 1.0),
            "{name} edge has blue on it: {blue:?}"
        );
        let thickness = blue.len() as f64 / scale;
        assert!(
            (1.0..=3.0).contains(&thickness),
            "{name} edge is a 1-3 px line, got {thickness} px"
        );
    }
    let inset = 4.0 * scale;
    for (x, y) in [
        (left + inset, middle_y),
        (right - inset, middle_y),
        (middle_x, top + inset),
        (middle_x, bottom - inset),
        (left - inset, middle_y),
        (middle_x, bottom + inset),
    ] {
        assert_eq!(hovered.at(x, y), plain.at(x, y), "untinted at ({x}, {y})");
    }
}

pub(super) fn overlay_event(lane: &mut Lane, method: &str) -> Option<Value> {
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        let events = lane.capability.port_mut().transport.take_overlay_events();
        if let Some(event) = events.into_iter().find(|event| event["method"] == method) {
            return Some(event["params"].clone());
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    None
}

#[test]
fn annotate_highlights_the_hovered_element_natively_and_leaving_clears_it() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    lane.capability.port_mut().transport.take_overlay_events();
    let (_, _, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x, y);
    let plain = capture(&mut lane);

    lane.capability
        .port_mut()
        .start_inspect()
        .expect("start inspect mode");
    let (node, backend, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x + 1.0, y);
    let hovered = overlay_event(&mut lane, "Overlay.nodeHighlightRequested")
        .expect("hover asks for a highlight");
    assert_eq!(hovered["nodeId"].as_u64(), Some(node));
    assert_eq!(capture(&mut lane), plain, "the inspect highlight is clear");
    mouse(&mut lane, "mouseMoved", x + 3.0, y);
    assert!(overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none());

    // What `devtools/picks.rs` does with the event on WebView2.
    let named = crate::browser::devtools::cdp_port::inspect::hovered_node(&hovered.to_string())
        .expect("the event names the node");
    lane.capability
        .port_mut()
        .outline_hovered(named)
        .expect("outline the hovered button");
    let quad =
        cdp(&mut lane, "DOM.getBoxModel", json!({"nodeId": node}))["model"]["border"].clone();
    let corner = |index: usize| quad[index].as_f64().unwrap();
    let outlined = Pixels::of(&capture(&mut lane));
    let unmarked = Pixels::of(&plain);
    let scale = capture_scale(&mut lane, &unmarked);
    assert_outlined(
        &outlined,
        &unmarked,
        scale,
        [corner(0), corner(1), corner(4), corner(5)],
    );

    mouse(&mut lane, "mousePressed", x + 1.0, y);
    mouse(&mut lane, "mouseReleased", x + 1.0, y);
    let clicked = overlay_event(&mut lane, "Overlay.inspectNodeRequested")
        .expect("a click in inspect mode names the node");
    assert_eq!(clicked["backendNodeId"].as_u64(), Some(backend));
    assert!(!lane.wait_text("Pressed for real", 300));
    assert_main_world_untouched(&mut lane);

    lane.capability
        .port_mut()
        .stop_inspect()
        .expect("stop inspect mode");
    mouse(&mut lane, "mouseMoved", x, y);
    assert_eq!(
        capture(&mut lane),
        plain,
        "leaving inspect mode clears the overlay"
    );
    mouse(&mut lane, "mouseMoved", x + 2.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none(),
        "no hover highlight after leaving"
    );
    assert_main_world_untouched(&mut lane);

    mouse(&mut lane, "mousePressed", x, y);
    mouse(&mut lane, "mouseReleased", x, y);
    assert!(lane.wait_text("Pressed for real", 2_000));
    assert!(
        lane.capability
            .port_mut()
            .transport
            .take_overlay_events()
            .is_empty()
    );
}

/// The inspect highlight's own container outline takes the renderer down on
/// an inline element, such as a link in running text.
#[test]
fn an_inline_element_gets_the_outline_too() {
    let mut lane = Lane::open("/hostile.html");
    watch_main_world(&mut lane);
    let plain = Pixels::of(&capture(&mut lane));
    lane.capability.port_mut().start_inspect().unwrap();
    lane.capability.port_mut().transport.take_overlay_events();
    let root = cdp(&mut lane, "DOM.getDocument", json!({"depth": 0}));
    let span = cdp(
        &mut lane,
        "DOM.querySelector",
        json!({"nodeId": root["root"]["nodeId"], "selector": "#picked"}),
    )["nodeId"]
        .as_u64()
        .expect("inline span");
    let quad =
        cdp(&mut lane, "DOM.getBoxModel", json!({"nodeId": span}))["model"]["border"].clone();
    let corner = |index: usize| quad[index].as_f64().unwrap();
    let border = [corner(0), corner(1), corner(4), corner(5)];
    mouse(
        &mut lane,
        "mouseMoved",
        border[0] + 2.0,
        (border[1] + border[3]) / 2.0,
    );
    let hovered = overlay_event(&mut lane, "Overlay.nodeHighlightRequested")
        .expect("hover asks for a highlight");
    assert_eq!(hovered["nodeId"].as_u64(), Some(span));
    lane.capability.port_mut().outline_hovered(span).unwrap();
    let outlined = Pixels::of(&capture(&mut lane));
    let scale = capture_scale(&mut lane, &plain);
    assert_outlined(&outlined, &plain, scale, border);

    lane.capability.port_mut().stop_inspect().unwrap();
    assert_eq!(
        Pixels::of(&capture(&mut lane)).rgb,
        plain.rgb,
        "leaving clears the outline"
    );
    assert_main_world_untouched(&mut lane);
}

#[test]
fn annotate_and_the_agent_highlight_share_one_overlay() {
    let mut lane = Lane::open("/hostile.html");
    let (_, _, x, y) = press_button(&mut lane);
    mouse(&mut lane, "mouseMoved", x, y);
    let plain = capture(&mut lane);
    lane.capability.port_mut().start_inspect().unwrap();
    let (_, backend, x, y) = press_button(&mut lane);
    lane.capability.port_mut().transport.take_overlay_events();
    // The parameters `cdp_port/highlight.rs` sends for an Agent act.
    let agent = json!({
        "backendNodeId": backend,
        "highlightConfig": {
            "showInfo": false,
            "contentColor": {"r": 74, "g": 143, "b": 255, "a": 0.24},
            "borderColor": {"r": 74, "g": 143, "b": 255, "a": 0.9}
        }
    });
    cdp(&mut lane, "Overlay.highlightNode", agent.clone());
    mouse(&mut lane, "mouseMoved", x + 3.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_none(),
        "an Agent highlight pauses hover highlighting"
    );
    cdp(&mut lane, "Overlay.hideHighlight", json!({}));
    mouse(&mut lane, "mouseMoved", x + 1.0, y);
    assert!(
        overlay_event(&mut lane, "Overlay.nodeHighlightRequested").is_some(),
        "clearing the Agent highlight resumes hover highlighting"
    );

    cdp(&mut lane, "Overlay.highlightNode", agent);
    lane.capability.port_mut().stop_inspect().unwrap();
    mouse(&mut lane, "mouseMoved", x, y);
    assert_eq!(
        capture(&mut lane),
        plain,
        "leaving Annotate clears the Agent highlight"
    );
}
