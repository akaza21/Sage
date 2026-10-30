use super::{CdpPort, CdpTransport};
use crate::{
    acp::browser::BrowserFailure,
    browser::annotate::{area::AreaPage, element::Rect, selection::Frame},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, Instant};

const DRAG: &str = include_str!("../../../../../src/features/browser/annotateDrag.js");
const READ: &str = include_str!("../../../../../src/features/browser/readAnnotateArea.js");
const BADGES: &str = include_str!("../../../../../src/features/browser/annotationBadges.js");
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AnnotationInput {
    pub target: InputTarget,
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum InputTarget {
    Element,
    Area,
}

impl AnnotationInput {
    pub(crate) fn admitted(&self) -> bool {
        [self.x, self.y, self.width, self.height]
            .iter()
            .all(|value| value.is_finite() && (0.0..=100_000.0).contains(value))
            && match self.target {
                InputTarget::Element => self.width == 0.0 && self.height == 0.0,
                InputTarget::Area => self.width >= 4.0 && self.height >= 4.0,
            }
    }
}

pub(crate) fn binding_input(payload: &str, name: &str, context: u64) -> Option<AnnotationInput> {
    if payload.len() > 4096 {
        return None;
    }
    let event: Value = serde_json::from_str(payload).ok()?;
    if event["name"].as_str() != Some(name) || event["executionContextId"].as_u64() != Some(context)
    {
        return None;
    }
    let payload = event["payload"]
        .as_str()
        .filter(|value| value.len() <= 512)?;
    let input: AnnotationInput = serde_json::from_str(payload).ok()?;
    input.admitted().then_some(input)
}

impl<T: CdpTransport> CdpPort<T> {
    pub(crate) fn annotation_context(&self) -> Result<u64, BrowserFailure> {
        self.in_agent_world(
            Some(self.stamp()?.navigation_id),
            true,
            Instant::now() + TIMEOUT,
            |context, _| Ok(context),
        )
    }

    pub(crate) fn install_annotation_input(
        &self,
        context: u64,
        binding: &str,
    ) -> Result<(), BrowserFailure> {
        self.input_call("Runtime.enable", json!({}))?;
        self.input_call(
            "Runtime.addBinding",
            json!({"name": binding, "executionContextId": context}),
        )?;
        let binding = serde_json::to_string(binding).map_err(|_| BrowserFailure::Unavailable)?;
        let function = DRAG
            .strip_prefix("export default ")
            .ok_or(BrowserFailure::Unavailable)?;
        self.input_call("Runtime.evaluate", json!({
            "expression": format!("globalThis.__sageAnnotationInput?.(); globalThis.__sageAnnotationInput = ({function})((payload) => globalThis[{binding}](payload)); globalThis.__sageAnnotationBadges?.resume(); void 0"),
            "contextId": context, "returnByValue": true,
        })).map(drop)
    }

    pub(crate) fn annotation_input_state(&self, action: &str) -> Result<(), BrowserFailure> {
        let mut expression = if action == "stop" {
            "globalThis.__sageAnnotationInput?.(); globalThis.__sageAnnotationBadges?.resume(); void 0".to_owned()
        } else {
            format!("globalThis.__sageAnnotationInput?.{action}(); void 0")
        };
        if action == "pause" {
            expression.push_str("; globalThis.__sageAnnotationBadges?.pause(); void 0");
        }
        self.evaluate_in_agent_world(
            Some(self.stamp()?.navigation_id),
            &expression,
            4096,
            Instant::now() + TIMEOUT,
        )
        .map(drop)
    }

    pub(crate) fn annotation_badges(
        &self,
        annotations: &[crate::browser::annotate::element::BrowserElement],
        url: &str,
        paused: bool,
    ) -> Result<(), BrowserFailure> {
        let entries: Vec<_> = annotations
            .iter()
            .map(|item| json!({"ordinal":item.ordinal, "box":item.bounds}))
            .collect();
        let function = BADGES
            .strip_prefix("export default ")
            .ok_or(BrowserFailure::Unavailable)?;
        let expression = format!(
            "({function})({}, {}); {}; void 0",
            json!(entries),
            json!(url),
            if paused {
                "globalThis.__sageAnnotationBadges?.pause()"
            } else {
                ""
            }
        );
        self.evaluate_in_agent_world(
            Some(self.stamp()?.navigation_id),
            &expression,
            4096,
            Instant::now() + TIMEOUT,
        )
        .map(drop)
    }

    pub(crate) fn annotation_node(&self, input: AnnotationInput) -> Result<u64, BrowserFailure> {
        if !input.admitted() || input.target != InputTarget::Element {
            return Err(BrowserFailure::InvalidRequest);
        }
        self.annotation_input_state("pause")?;
        self.input_call("DOM.getNodeForLocation", json!({"x": input.x.round() as i64, "y": input.y.round() as i64, "includeUserAgentShadowDOM": false}))?["backendNodeId"].as_u64().filter(|id| *id > 0).ok_or(BrowserFailure::Unavailable)
    }

    pub(crate) fn capture_area(
        &self,
        input: AnnotationInput,
    ) -> Result<(AreaPage, Rect, Frame, bool), BrowserFailure> {
        if !input.admitted() || input.target != InputTarget::Area {
            return Err(BrowserFailure::InvalidRequest);
        }
        let before = self.stamp()?;
        self.annotation_input_state("pause")?;
        let read = READ
            .strip_prefix("export default ")
            .ok_or(BrowserFailure::Unavailable)?;
        let expression = format!(
            "({read})({})",
            serde_json::to_string(&input).map_err(|_| BrowserFailure::InvalidRequest)?
        );
        let value = self.evaluate_in_agent_world(
            Some(before.navigation_id),
            &expression,
            262_144,
            Instant::now() + TIMEOUT,
        )?;
        let page: AreaPage = serde_json::from_value(value).map_err(|_| BrowserFailure::Capacity)?;
        let rect = Rect {
            x: input.x + page.viewport.x,
            y: input.y + page.viewport.y,
            width: input.width,
            height: input.height,
        };
        let before_coverage = self.annotation_mask_coverage();
        // Use lossless pixels: JPEG DCT blocks can carry control glyphs
        // beyond their masks before the crop is redacted and encoded.
        let frame = self.capture_area_frame()?;
        let after = self.evaluate_in_agent_world(
            Some(before.navigation_id),
            &expression,
            262_144,
            Instant::now() + TIMEOUT,
        )?;
        let after: AreaPage =
            serde_json::from_value(after).map_err(|_| BrowserFailure::Capacity)?;
        if after.viewport != page.viewport || after.masks != page.masks {
            return Err(BrowserFailure::StaleHandle);
        }
        if self.stamp()? != before {
            return Err(BrowserFailure::StaleHandle);
        }
        let coverage = before_coverage && self.annotation_mask_coverage();
        if self.stamp()? != before {
            return Err(BrowserFailure::StaleHandle);
        }
        Ok((page, rect, frame, coverage))
    }

    fn input_call(&self, method: &str, params: Value) -> Result<Value, BrowserFailure> {
        let response = self
            .transport
            .observe(method, &params.to_string(), 4096, TIMEOUT)?;
        let value: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        if value.get("exceptionDetails").is_some() {
            return Err(BrowserFailure::Unavailable);
        }
        Ok(value)
    }

    fn capture_area_frame(&self) -> Result<Frame, BrowserFailure> {
        use base64::{Engine, engine::general_purpose::STANDARD};
        let response = self.transport.observe(
            "Page.captureScreenshot",
            &json!({"format":"png", "fromSurface":true, "captureBeyondViewport":false}).to_string(),
            8_004_096,
            TIMEOUT,
        )?;
        let value: Value =
            serde_json::from_str(&response).map_err(|_| BrowserFailure::Unavailable)?;
        let data = value["data"]
            .as_str()
            .filter(|data| data.len() <= 8_000_000)
            .ok_or(BrowserFailure::Capacity)?;
        let bytes = STANDARD
            .decode(data)
            .map_err(|_| BrowserFailure::Unavailable)?;
        let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
        decoder.set_limits(png::Limits { bytes: 67_108_864 });
        let reader = decoder
            .read_info()
            .map_err(|_| BrowserFailure::Unavailable)?;
        let (width, height) = (reader.info().width, reader.info().height);
        if width == 0 || height == 0 || u64::from(width) * u64::from(height) > 16_777_216 {
            return Err(BrowserFailure::Capacity);
        }
        Ok(Frame {
            jpeg_base64: String::new(),
            png_base64: Some(data.to_owned()),
            width,
            height,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn capture_reports_require_the_scoped_isolated_binding_and_bounded_geometry() {
        let input = json!({"target":"area", "x":10, "y":20, "width":300, "height":200}).to_string();
        let report =
            json!({"name":"privateBinding", "executionContextId":17, "payload":input}).to_string();
        assert!(binding_input(&report, "privateBinding", 17).is_some());
        assert!(binding_input(&report, "pageBinding", 17).is_none());
        assert!(binding_input(&report, "privateBinding", 0).is_none());
        assert!(binding_input(&"x".repeat(4097), "privateBinding", 17).is_none());
        for width in [0.0, 3.0, 100_001.0, f64::NAN, f64::INFINITY] {
            assert!(
                !AnnotationInput {
                    target: InputTarget::Area,
                    x: 10.0,
                    y: 20.0,
                    width,
                    height: 200.0
                }
                .admitted()
            );
        }
    }
}
