//! Developer mode (ADR-0009b): the checks a raw CDP command passes before it
//! may reach the attached tab, and the seam through which the gate reads the
//! Settings switch, the per-session grant and the audit log.
//!
//! Default deny: only the methods in `ALLOWED`, each with only its listed
//! top-level parameters, are sent. Every one of them runs in the main
//! frame's default execution context (where the same-origin policy keeps
//! cross-origin iframes out) or navigates or captures the tab, and none
//! enables a domain. Native raw calls use a grant-owned session detached on
//! revocation, sleep and timeout. The `DOM` domain is left out because its
//! node ids reach same-process iframe documents and it enables itself;
//! cookie commands are left out because they hand over `HttpOnly` session
//! tokens that page script cannot read. Frame and context ids are never
//! accepted, and neither is a remote `objectId`: it encodes the execution
//! context it belongs to, so a hand-built one could name an iframe's
//! context. Results come back by value or as previews.

use serde_json::{Map, Value};

use super::BrowserFailure;
mod screenshot;
pub(crate) use screenshot::ScreenshotCapture;
mod switch;
pub(crate) use switch::{switch_generation, switch_is_current, with_switch_change};

/// `Runtime.evaluate` parameters the Agent may set. Sage itself sets
/// `returnByValue: false` and its own `objectGroup`. Objects are serialized
/// within a byte budget and the group is released after every call; previews and
/// `serializationOptions` (whose `idOnly` overrides `returnByValue`) would
/// keep remote objects alive, so they are not accepted.
const EVALUATE: &[&str] = &[
    "expression",
    "silent",
    "userGesture",
    "awaitPromise",
    "throwOnSideEffect",
    "timeout",
    "disableBreaks",
    "replMode",
];

/// The object group every developer-mode evaluation runs in; the native
/// port releases it after each call and when a grant ends.
pub const DEVELOPER_OBJECT_GROUP: &str = "sage-developer-mode";

/// V8's synchronous evaluation limit, below the native 30 s deadline.
/// `awaitPromise` can outlive V8's limit; native timeout detaches its session.
pub const EVALUATE_TIMEOUT_MS: u64 = 25_000;

/// Whether this completion owns developer objects and needs cleanup. A timely
/// by-reference evaluation keeps its handle until the fixed serializer finishes;
/// failed/late evaluations and serializer completions release the group.
pub fn releases_on_completion(method: &str, params: &str) -> bool {
    matches!(method, "Runtime.evaluate" | "Runtime.callFunctionOn")
        && serde_json::from_str::<Value>(params)
            .is_ok_and(|params| params["objectGroup"] == DEVELOPER_OBJECT_GROUP)
}

/// Params for `Runtime.releaseObjectGroup` on developer mode's group.
pub fn release_params() -> String {
    serde_json::json!({ "objectGroup": DEVELOPER_OBJECT_GROUP }).to_string()
}

/// Every method developer mode sends, with the parameters it may carry.
const ALLOWED: &[(&str, &[&str])] = &[
    ("Runtime.evaluate", EVALUATE),
    ("Runtime.getHeapUsage", &[]),
    (
        "Page.navigate",
        &["url", "referrer", "transitionType", "referrerPolicy"],
    ),
    ("Page.reload", &["ignoreCache"]),
    ("Page.stopLoading", &[]),
    ("Page.getFrameTree", &[]),
    ("Page.getNavigationHistory", &[]),
    ("Page.getLayoutMetrics", &[]),
    (
        "Page.captureScreenshot",
        &[
            "format",
            "quality",
            "clip",
            "fromSurface",
            "captureBeyondViewport",
            "optimizeForSpeed",
        ],
    ),
];

/// Capability text and schemas use the same source as command admission.
pub fn allowed_methods() -> Vec<&'static str> {
    ALLOWED.iter().map(|(method, _)| *method).collect()
}

/// Accepts `Domain.command`, both ASCII alphanumeric and non-empty.
pub fn admit_method_shape(method: &str) -> Result<(), BrowserFailure> {
    let valid_part =
        |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_alphanumeric());
    match method.split_once('.') {
        Some((domain, command))
            if method.len() <= 128 && valid_part(domain) && valid_part(command) =>
        {
            Ok(())
        }
        _ => Err(BrowserFailure::InvalidRequest),
    }
}

/// Refuses any method not in `ALLOWED` and any parameter not listed for
/// it. The caller has already checked the method's shape. `origin` is the
/// granted origin; no listed method takes an origin-bearing parameter
/// today, so none needs binding to it.
pub fn admit_command(
    method: &str,
    params: &Map<String, Value>,
    _origin: &str,
) -> Result<(), BrowserFailure> {
    let Some((_, keys)) = ALLOWED.iter().find(|(allowed, _)| *allowed == method) else {
        return Err(BrowserFailure::Denied);
    };
    if !params.keys().all(|key| keys.contains(&key.as_str())) {
        return Err(BrowserFailure::Denied);
    }
    if method == "Page.captureScreenshot" {
        ScreenshotCapture::parse(params)?;
    }
    Ok(())
}

/// How one developer-mode call ended, as the audit log records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpAuditOutcome {
    Granted,
    Revoked,
    /// Approved and about to be sent; written before the command leaves
    /// Sage, which refuses to send it if the row cannot be written.
    Sent,
    Ok,
    /// The switch was off, the user declined, or the grant had ended.
    Denied,
    /// The deny list or an origin check refused it before any card.
    Blocked,
    Failed(BrowserFailure),
}

impl CdpAuditOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Granted => "granted",
            Self::Revoked => "revoked",
            Self::Sent => "sent",
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Blocked => "blocked",
            Self::Failed(BrowserFailure::Capacity) => "capacity",
            Self::Failed(BrowserFailure::OutcomeUnknown) => "outcomeUnknown",
            Self::Failed(
                BrowserFailure::Denied | BrowserFailure::NoAttachment | BrowserFailure::UserControl,
            ) => "denied",
            Self::Failed(BrowserFailure::InvalidRequest) => "blocked",
            Self::Failed(_) => "unavailable",
        }
    }
}

/// One audit attempt. The writer persists only trusted metadata and byte
/// counts; parameter values are never stored. No result content is stored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CdpAuditEntry {
    pub session_id: String,
    pub tab_id: u64,
    pub origin: String,
    pub method: String,
    pub params: String,
    pub result_bytes: Option<usize>,
    pub outcome: CdpAuditOutcome,
}

/// What the permission gate needs to admit an elevated command. Grants are
/// held in memory by the attachment and end on Stop, Take over, detach,
/// tab close, a cross-origin commit and the switch going off.
pub trait DeveloperModePolicy: Send + Sync {
    /// Whether Settings › Browser › Developer mode is on.
    fn enabled(&self) -> bool;
    /// Whether `session` holds a live grant for `origin` on this tab.
    fn granted(&self, session: &str, origin: &str) -> bool;
    /// Records a grant the user just approved; false when the attachment
    /// can no longer hold one.
    fn grant(&self, session: &str, origin: &str) -> bool;
    /// Writes one audit row; false when it could not be written.
    fn audit(&self, entry: CdpAuditEntry) -> bool;
}

#[cfg(test)]
#[path = "developer/tests.rs"]
mod tests;
