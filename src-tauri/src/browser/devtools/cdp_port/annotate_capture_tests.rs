use super::*;
use crate::browser::devtools::{snapshot::DocumentStamp, transport::Settled};
use std::sync::Mutex;

#[derive(Default)]
struct MovingPage {
    state: Mutex<(bool, f64, bool, bool)>,
    fail: Option<&'static str>,
    motion_unsafe: bool,
}

impl CdpTransport for MovingPage {
    fn observe(
        &self,
        method: &str,
        params: &str,
        _: usize,
        _: Duration,
    ) -> Result<String, BrowserFailure> {
        let params: Value = serde_json::from_str(params).unwrap();
        let mut state = self.state.lock().unwrap();
        if self.fail == Some(method) {
            return Err(BrowserFailure::Unavailable);
        }
        match method {
            "Input.setIgnoreInputEvents" => { state.3 = params["ignore"] == true; Ok("{}".into()) },
            "Target.getTargetInfo" => Ok(json!({"targetInfo":{"targetId":"main"}}).to_string()),
            "Page.createIsolatedWorld" => Ok(json!({"executionContextId":17}).to_string()),
            "Emulation.setScriptExecutionDisabled" => { state.0 = params["value"] == true; Ok("{}".into()) },
            "Animation.getPlaybackRate" => Ok(json!({"playbackRate":state.1}).to_string()),
            "Animation.setPlaybackRate" => { state.1 = params["playbackRate"].as_f64().unwrap(); Ok("{}".into()) },
            "Runtime.evaluate" if params["expression"].as_str().unwrap().contains("function readAnnotateArea") => {
                Ok(json!({"result":{"value":{
                    "title":"Example", "nodes":[], "masks":[{"x":0,"y":0,"width":1,"height":1}], "motionSafe":!self.motion_unsafe,
                    "viewport":{"x":0,"y":0,"width":20,"height":20,"clientWidth":20,"clientHeight":20,"scale":1}
                }}}).to_string())
            },
            "Runtime.evaluate" => Ok(json!({"result":{"value":true}}).to_string()),
            "DOM.getDocument" => Ok(json!({"root":{"nodeName":"HTML"}}).to_string()),
            "Page.captureScreenshot" => {
                // Before/after samples agree, but a timer or compositor animation
                // moves the secret control between them unless BOTH are held.
                state.2 = !state.0 || state.1 != 0.0 || !state.3;
                use base64::{Engine, engine::general_purpose::STANDARD};
                let mut bytes = Vec::new();
                let mut encoder = png::Encoder::new(&mut bytes, 20, 20);
                encoder.set_color(png::ColorType::Rgb);
                encoder.set_depth(png::BitDepth::Eight);
                encoder.write_header().unwrap().write_image_data(&[255;1200]).unwrap();
                Ok(json!({"data":STANDARD.encode(bytes)}).to_string())
            },
            _ => panic!("unexpected {method}"),
        }
    }
    fn call(&self, _: &str, _: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
        unreachable!()
    }
    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        Ok(DocumentStamp::default())
    }
    fn issued(&self) -> bool {
        false
    }
    fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        Ok(None)
    }
}

fn input() -> AnnotationInput {
    AnnotationInput {
        target: InputTarget::Area,
        x: 0.0,
        y: 0.0,
        width: 20.0,
        height: 20.0,
    }
}

#[test]
fn capture_blocks_transient_control_movement_even_when_geometry_samples_agree() {
    let port = CdpPort::new(MovingPage {
        state: Mutex::new((false, 0.5, false, false)),
        ..MovingPage::default()
    });
    assert!(port.capture_area(input()).unwrap().3);
    let state = *port.transport.state.lock().unwrap();
    assert!(
        !state.2,
        "capture exposed a control between identical mask samples"
    );
    assert_eq!((state.0, state.1), (false, 0.5));
    assert!(!state.3);
}

#[test]
fn capture_fails_closed_and_restores_page_on_capture_or_freeze_failure() {
    for fail in [
        "Page.captureScreenshot",
        "Animation.setPlaybackRate",
        "Emulation.setScriptExecutionDisabled",
        "Input.setIgnoreInputEvents",
    ] {
        let port = CdpPort::new(MovingPage {
            state: Mutex::new((false, 0.5, false, false)),
            fail: Some(fail),
            ..MovingPage::default()
        });
        assert!(port.capture_area(input()).is_err(), "{fail}");
        let state = *port.transport.state.lock().unwrap();
        assert_eq!((state.0, state.1), (false, 0.5), "{fail}");
        assert!(!state.3);
    }
}

#[test]
fn capture_keeps_area_text_but_refuses_pictures_for_unheld_timelines() {
    let port = CdpPort::new(MovingPage {
        motion_unsafe: true,
        ..MovingPage::default()
    });
    assert!(!port.capture_area(input()).unwrap().3);
}
