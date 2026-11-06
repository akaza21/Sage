use std::time::Duration;

use crate::acp::browser::BrowserFailure;

use super::snapshot::DocumentStamp;

/// Native transport envelope for Sage-built requests, independent of the
/// smaller agent parameter allowance enforced by the browser contract.
pub(super) const INTERNAL_REQUEST_BYTES: usize = 16_384;

pub(super) fn admit_request(method: &str, params: &str) -> Result<(), BrowserFailure> {
    if method.len() > 128 || params.len() > INTERNAL_REQUEST_BYTES {
        return Err(BrowserFailure::Capacity);
    }
    Ok(())
}

/// A main-frame document that finished loading after an Agent navigation.
pub(crate) struct Settled {
    pub url: String,
    pub navigation_id: u64,
}

/// Carries CDP commands to one page. The command logic in `cdp_port` builds
/// every parameter and parses every result; a transport only moves bytes and
/// reports which document the page currently shows.
pub(crate) trait CdpTransport {
    /// Bitmap scale for the raw command's session, including its own device
    /// override, excluding browser zoom and other sessions' overrides.
    /// Unknown or changing capture settings fail closed.
    fn screenshot_scale(&self, _timeout: Duration) -> Result<f64, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }

    /// Issues one Agent command, under the dispatch permit when one is held.
    /// The result is the CDP `result` object as JSON, at most
    /// `max_result_bytes` long.
    fn call(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure>;

    /// Privileged calls use a grant-owned native session. Test transports
    /// already isolate their target and may use their normal command lane.
    fn raw(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure> {
        self.call(method, params, max_result_bytes, timeout)
    }

    fn release_raw_objects(&self) {
        let _ = self.observe(
            "Runtime.releaseObjectGroup",
            &crate::acp::browser::developer::release_params(),
            4096,
            Duration::from_secs(2),
        );
    }

    /// Reads page state without the dispatch permit: after an Agent command
    /// was issued, and for wait probes and page-context reads, which also
    /// make and check the Agent's isolated world this way. It never consumes
    /// or waits on the permit.
    fn observe(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure>;

    /// The committed document the next command would act on.
    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure>;

    /// Whether an Agent command left this transport in the current dispatch.
    fn issued(&self) -> bool;

    /// Whether the dispatch permit was stopped (Stop, Take over, detach)
    /// rather than spent on a navigation; a transport with no permit is
    /// never stopped. Loops that only read, such as a wait, end on it.
    fn stopped(&self) -> bool {
        false
    }

    /// Whether the user's Annotate overlay is on this page, which needs the
    /// DOM and Overlay domains kept on. A lock-free read, for drawing only.
    fn annotating(&self) -> bool {
        false
    }

    /// Runs `steps` only while this page does not annotate and no Annotate
    /// change is under way, so a start cannot enable its overlay between the
    /// check and `steps`. It never waits; when Annotate is busy it skips.
    fn unless_annotating(&self, steps: &mut dyn FnMut()) {
        if !self.annotating() {
            steps();
        }
    }

    /// Polls once for a settled document newer than `before`. `Ok(None)`
    /// means still loading; an error ends the navigation wait.
    fn settled(&self, before: DocumentStamp) -> Result<Option<Settled>, BrowserFailure>;
}
