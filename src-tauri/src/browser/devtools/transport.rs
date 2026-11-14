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

pub(crate) struct Settled {
    pub url: String,
    pub navigation_id: u64,
}

pub(crate) trait CdpTransport {
    /// Bitmap scale for the raw command's session, including its own device
    /// override, excluding browser zoom and other sessions' overrides.
    /// Unknown or changing capture settings fail closed.
    fn screenshot_scale(&self, _timeout: Duration) -> Result<f64, BrowserFailure> {
        Err(BrowserFailure::Unavailable)
    }

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

    /// Never consumes or waits on the dispatch permit.
    fn observe(
        &self,
        method: &str,
        params: &str,
        max_result_bytes: usize,
        timeout: Duration,
    ) -> Result<String, BrowserFailure>;

    fn stamp(&self) -> Result<DocumentStamp, BrowserFailure>;

    fn issued(&self) -> bool;

    fn stopped(&self) -> bool {
        false
    }

    /// A lock-free read, for drawing only.
    fn annotating(&self) -> bool {
        false
    }

    /// No Annotate change may be under way either, so a start cannot enable its
    /// overlay between the check and `steps`. It never waits; when Annotate is
    /// busy it skips.
    fn unless_annotating(&self, steps: &mut dyn FnMut()) {
        if !self.annotating() {
            steps();
        }
    }

    /// `Ok(None)` means still loading; an error ends the navigation wait.
    fn settled(&self, before: DocumentStamp) -> Result<Option<Settled>, BrowserFailure>;
}
