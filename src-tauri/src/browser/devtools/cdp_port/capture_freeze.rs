use super::{CdpPort, CdpTransport};
use crate::acp::browser::BrowserFailure;
use serde_json::json;

impl<T: CdpTransport> CdpPort<T> {
    /// Matching mask samples cannot detect a control moving out and back while
    /// Chromium captures. Disable page scripts AND the compositor animation timeline
    /// before reading geometry; restore both even when capture fails.
    pub(super) fn with_annotation_capture<R>(
        &self,
        capture: impl FnOnce() -> Result<R, BrowserFailure>,
    ) -> Result<R, BrowserFailure> {
        let rate = self.input_call("Animation.getPlaybackRate", json!({}))?["playbackRate"]
            .as_f64()
            .filter(|rate| rate.is_finite())
            .ok_or(BrowserFailure::Unavailable)?;
        // Lifecycle freezing alone is insufficient: captureScreenshot may thaw
        // timer tasks. Runtime.evaluate still permits our isolated-world reads.
        let result = self
            .input_call("Input.setIgnoreInputEvents", json!({"ignore":true}))
            .and_then(|_| {
                self.input_call(
                    "Emulation.setScriptExecutionDisabled",
                    json!({"value":true}),
                )
            })
            .and_then(|_| self.input_call("Animation.setPlaybackRate", json!({"playbackRate":0})))
            .and_then(|_| capture());
        let timeline = self.input_call("Animation.setPlaybackRate", json!({"playbackRate":rate}));
        let tasks = self.input_call(
            "Emulation.setScriptExecutionDisabled",
            json!({"value":false}),
        );
        let input = self.input_call("Input.setIgnoreInputEvents", json!({"ignore":false}));
        // A failed hold or restore never yields usable pixels.
        result.and_then(|value| timeline.and(tasks).and(input).map(|_| value))
    }
}
