//! Developer mode (ADR-0009b): the checks a raw CDP command passes before it
//! may reach the attached tab, and the seam through which the gate reads the
//! Settings switch, the per-session grant and the audit log.
//!
//! Default deny: only the methods in `ALLOWED`, each with only its listed
//! top-level parameters, are sent. Every one of them acts on the main
//! frame's document or default execution context, reads or changes this
//! origin's cookies, or navigates or captures the tab, and none leaves state
//! on the tab's shared DevTools session after the grant ends. Node handles
//! are only `nodeId` (issued for the main document, never pierced) and
//! `objectId` (from the main world); `backendNodeId`, frame and context ids
//! are never accepted because they can name same-process iframes.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value};

use super::BrowserFailure;

const NODE: &[&str] = &["nodeId", "objectId"];
const EVALUATE: &[&str] = &[
    "expression",
    "objectGroup",
    "silent",
    "returnByValue",
    "generatePreview",
    "userGesture",
    "awaitPromise",
    "throwOnSideEffect",
    "timeout",
    "disableBreaks",
    "replMode",
    "serializationOptions",
];
const CALL_FUNCTION: &[&str] = &[
    "functionDeclaration",
    "objectId",
    "arguments",
    "silent",
    "returnByValue",
    "generatePreview",
    "userGesture",
    "awaitPromise",
    "objectGroup",
    "throwOnSideEffect",
    "serializationOptions",
];
const COOKIE: &[&str] = &[
    "name",
    "value",
    "url",
    "domain",
    "path",
    "secure",
    "httpOnly",
    "sameSite",
    "expires",
    "priority",
    "sourceScheme",
    "sourcePort",
];

/// Every method developer mode sends, with the parameters it may carry.
const ALLOWED: &[(&str, &[&str])] = &[
    ("Runtime.evaluate", EVALUATE),
    ("Runtime.callFunctionOn", CALL_FUNCTION),
    (
        "Runtime.getProperties",
        &[
            "objectId",
            "ownProperties",
            "accessorPropertiesOnly",
            "generatePreview",
            "nonIndexedPropertiesOnly",
        ],
    ),
    (
        "Runtime.awaitPromise",
        &["promiseObjectId", "returnByValue", "generatePreview"],
    ),
    ("Runtime.releaseObject", &["objectId"]),
    ("Runtime.releaseObjectGroup", &["objectGroup"]),
    ("Runtime.getHeapUsage", &[]),
    ("DOM.getDocument", &["depth"]),
    ("DOM.querySelector", &["nodeId", "selector"]),
    ("DOM.querySelectorAll", &["nodeId", "selector"]),
    ("DOM.describeNode", &["nodeId", "objectId", "depth"]),
    ("DOM.getOuterHTML", NODE),
    ("DOM.getAttributes", &["nodeId"]),
    ("DOM.getBoxModel", NODE),
    ("DOM.getContentQuads", NODE),
    ("DOM.resolveNode", &["nodeId", "objectGroup"]),
    ("DOM.requestNode", &["objectId"]),
    ("DOM.setAttributeValue", &["nodeId", "name", "value"]),
    ("DOM.setAttributesAsText", &["nodeId", "text", "name"]),
    ("DOM.removeAttribute", &["nodeId", "name"]),
    ("DOM.setNodeValue", &["nodeId", "value"]),
    ("DOM.setOuterHTML", &["nodeId", "outerHTML"]),
    ("DOM.removeNode", &["nodeId"]),
    ("DOM.focus", NODE),
    (
        "DOM.scrollIntoViewIfNeeded",
        &["nodeId", "objectId", "rect"],
    ),
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
    ("Network.getCookies", &["urls"]),
    ("Network.setCookie", COOKIE),
    ("Network.setCookies", &["cookies"]),
    ("Network.deleteCookies", &["name", "url", "domain", "path"]),
];

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

/// Refuses any method not in `ALLOWED`, any parameter not listed for it,
/// and cookie commands that leave the granted origin. The caller has
/// already checked the method's shape.
pub fn admit_command(
    method: &str,
    params: &Map<String, Value>,
    origin: &str,
) -> Result<(), BrowserFailure> {
    let Some((_, keys)) = ALLOWED.iter().find(|(allowed, _)| *allowed == method) else {
        return Err(BrowserFailure::Denied);
    };
    if !params.keys().all(|key| keys.contains(&key.as_str())) {
        return Err(BrowserFailure::Denied);
    }
    match method {
        // Without `urls`, CDP answers for every frame, cross-origin ones too.
        "Network.getCookies" => match params.get("urls") {
            Some(Value::Array(urls)) if !urls.is_empty() => urls
                .iter()
                .all(|url| url.as_str().is_some_and(|url| same_origin(url, origin)))
                .then_some(())
                .ok_or(BrowserFailure::Denied),
            _ => Err(BrowserFailure::Denied),
        },
        "Network.setCookie" | "Network.deleteCookies" => cookie_target(params, origin),
        "Network.setCookies" => match params.get("cookies") {
            Some(Value::Array(cookies)) => cookies.iter().try_for_each(|cookie| match cookie {
                Value::Object(cookie)
                    if cookie.keys().all(|key| COOKIE.contains(&key.as_str())) =>
                {
                    cookie_target(cookie, origin)
                }
                _ => Err(BrowserFailure::Denied),
            }),
            _ => Err(BrowserFailure::Denied),
        },
        _ => Ok(()),
    }
}

/// A cookie command must name the granted origin by `url`, and any `domain`
/// must be that origin's exact host: a leading dot or a parent domain would
/// reach sibling sites.
fn cookie_target(params: &Map<String, Value>, origin: &str) -> Result<(), BrowserFailure> {
    let Some(url) = params.get("url").and_then(Value::as_str) else {
        return Err(BrowserFailure::Denied);
    };
    if !same_origin(url, origin) {
        return Err(BrowserFailure::Denied);
    }
    match params.get("domain") {
        None => Ok(()),
        Some(Value::String(domain)) if host_of(origin).as_deref() == Some(domain.as_str()) => {
            Ok(())
        }
        Some(_) => Err(BrowserFailure::Denied),
    }
}

fn same_origin(url: &str, origin: &str) -> bool {
    url::Url::parse(url).is_ok_and(|url| url.origin().ascii_serialization() == origin)
}

fn host_of(origin: &str) -> Option<String> {
    url::Url::parse(origin)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
}

/// How one developer-mode call ended, as the audit log records it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CdpAuditOutcome {
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
            Self::Sent => "sent",
            Self::Ok => "ok",
            Self::Denied => "denied",
            Self::Blocked => "blocked",
            Self::Failed(BrowserFailure::Capacity) => "capacity",
            Self::Failed(BrowserFailure::OutcomeUnknown) => "outcomeUnknown",
            Self::Failed(BrowserFailure::Denied) => "denied",
            Self::Failed(_) => "unavailable",
        }
    }
}

/// One audit row. `params_preview` is redacted and truncated by the
/// writer; `result_bytes` is set only when a result came back.
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

/// Bumped whenever the Settings switch changes. A call captures it when it
/// starts; a grant or send made after the switch changed is refused, even
/// when the user approves a card that was already open.
static SWITCH_GENERATION: AtomicU64 = AtomicU64::new(0);

pub fn switch_generation() -> u64 {
    SWITCH_GENERATION.load(Ordering::SeqCst)
}

/// Called before the switch's grants are ended, so a grant racing the
/// switch either lands first and is ended, or sees the new generation.
pub fn bump_switch_generation() {
    SWITCH_GENERATION.fetch_add(1, Ordering::SeqCst);
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
