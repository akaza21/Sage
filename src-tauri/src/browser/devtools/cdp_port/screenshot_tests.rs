//! Behavioral pre-dispatch tests with a deterministic browser transport.
//! An issued capture succeeds, so Capacity can only come from preflight.
use serde_json::{Value, json};
use std::time::Duration;

use super::*;
use crate::browser::devtools::{
    capability::DevtoolsPort, snapshot::DocumentStamp, transport::Settled,
};

struct Page {
    viewport: (f64, f64),
    content: (f64, f64),
    ratio: Value,
    metrics_unavailable: bool,
    metrics_missing: bool,
    navigated: std::sync::atomic::AtomicBool,
    navigate_during_ratio: bool,
}

impl Default for Page {
    fn default() -> Self {
        Self {
            viewport: (1920.0, 1080.0),
            content: (16000.0, 16000.0),
            ratio: json!(1),
            metrics_unavailable: false,
            metrics_missing: false,
            navigated: false.into(),
            navigate_during_ratio: false,
        }
    }
}

impl CdpTransport for Page {
    fn call(&self, _: &str, params: &str, _: usize, _: Duration) -> Result<String, BrowserFailure> {
        let params: Value = serde_json::from_str(params).unwrap();
        // The page grows after preflight. An implicit full-page capture
        // would now exceed the bound, whereas an explicit clip stays fixed.
        let (width, height) = if let Some(clip) = params.get("clip") {
            (
                clip["width"].as_f64().unwrap(),
                clip["height"].as_f64().unwrap(),
            )
        } else {
            (16000.0, 16000.0)
        };
        Ok(json!({"width": width, "height": height}).to_string())
    }

    fn observe(
        &self,
        method: &str,
        _: &str,
        _: usize,
        _: Duration,
    ) -> Result<String, BrowserFailure> {
        Ok(match method {
            "Page.getLayoutMetrics" => {
                if self.metrics_unavailable {
                    return Err(BrowserFailure::Unavailable);
                }
                if self.metrics_missing {
                    return Ok("{}".into());
                }
                json!({
                    "cssVisualViewport": {"pageX": 0, "pageY": 0,
                        "clientWidth": self.viewport.0, "clientHeight": self.viewport.1},
                    "cssContentSize": {"x": 0, "y": 0,
                        "width": self.content.0, "height": self.content.1}
                })
            }
            "Target.getTargetInfo" => json!({"targetInfo": {"targetId": "main"}}),
            "Page.createIsolatedWorld" => json!({"executionContextId": 7}),
            "Runtime.evaluate" => {
                self.navigated.store(
                    self.navigate_during_ratio,
                    std::sync::atomic::Ordering::SeqCst,
                );
                json!({"result": {"value": self.ratio}})
            }
            _ => return Err(BrowserFailure::Unavailable),
        }
        .to_string())
    }

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure> {
        let navigation_id = if self.navigated.load(std::sync::atomic::Ordering::SeqCst) {
            2
        } else {
            1
        };
        Ok(DocumentStamp {
            navigation_id,
            generation: navigation_id,
        })
    }
    fn issued(&self) -> bool {
        false
    }
    fn settled(&self, _: DocumentStamp) -> Result<Option<Settled>, BrowserFailure> {
        Ok(None)
    }
}

fn capture(page: Page, params: Value) -> Result<Value, BrowserFailure> {
    let result = CdpPort::new(page).raw("Page.captureScreenshot", &params.to_string())?;
    Ok(serde_json::from_str(&result).unwrap())
}

#[test]
fn screenshots_without_a_clip_bound_the_viewport_or_full_page() {
    assert_eq!(
        capture(
            Page {
                viewport: (16000.0, 16000.0),
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        capture(Page::default(), json!({"captureBeyondViewport": true})),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        capture(Page::default(), json!({"captureBeyondViewport": false})),
        Ok(json!({"width": 1920.0, "height": 1080.0}))
    );
    assert_eq!(
        capture(
            Page {
                content: (1440.0, 1440.0),
                ..Page::default()
            },
            json!({"captureBeyondViewport": true})
        ),
        Ok(json!({"width": 1440.0, "height": 1440.0}))
    );
}

#[test]
fn screenshot_bounds_include_device_pixels_and_clip_scale() {
    let clip = json!({"clip": {"x": 0, "y": 0, "width": 1920, "height": 1080, "scale": 1}});
    assert_eq!(
        capture(
            Page {
                ratio: json!(2),
                ..Page::default()
            },
            clip.clone()
        ),
        Err(BrowserFailure::Capacity)
    );
    assert_eq!(
        capture(
            Page {
                ratio: json!(2),
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::Capacity)
    );
    let mut scaled = clip;
    scaled["clip"]["scale"] = json!(0.5);
    assert_eq!(
        capture(
            Page {
                ratio: json!(2),
                ..Page::default()
            },
            scaled
        ),
        Ok(json!({"width": 1920.0, "height": 1080.0}))
    );
}

#[test]
fn screenshot_preflight_fails_closed_when_page_geometry_is_unknown_or_changes() {
    assert_eq!(
        capture(
            Page {
                metrics_unavailable: true,
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::Unavailable)
    );
    assert_eq!(
        capture(
            Page {
                metrics_missing: true,
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::Unavailable)
    );
    assert_eq!(
        capture(
            Page {
                ratio: Value::Null,
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::Unavailable)
    );
    assert_eq!(
        capture(
            Page {
                navigate_during_ratio: true,
                ..Page::default()
            },
            json!({})
        ),
        Err(BrowserFailure::StaleHandle)
    );
}
