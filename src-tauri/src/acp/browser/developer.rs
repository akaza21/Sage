//! Developer mode (ADR-0009b): the checks a raw CDP command passes before it
//! may reach the attached tab, and the seam through which the gate reads the
//! Settings switch, the per-session grant and the audit log.
//!
//! The deny list is about leaving the tab and the grant, not about what the
//! page can do: any command that reaches other targets, the whole browser
//! profile, local files, another origin's frames or storage, or that leaves
//! state behind on the tab's shared DevTools session after the grant ends,
//! is refused before it is sent.

use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Map, Value};

use super::BrowserFailure;

/// Domains whose commands reach beyond the attached tab or its origin, or
/// leave state on the tab's DevTools session that would outlive the grant
/// (breakpoints and pauses, request interception, emulation overrides,
/// virtual authenticators).
const DENIED_DOMAINS: &[&str] = &[
    "Autofill",
    "BackgroundService",
    "BluetoothEmulation",
    "Browser",
    "Cast",
    "DOMDebugger",
    "DOMSnapshot",
    "Debugger",
    "DeviceAccess",
    "DeviceOrientation",
    "Emulation",
    "EventBreakpoints",
    "Extensions",
    "FedCm",
    "Fetch",
    "IO",
    "PWA",
    "ServiceWorker",
    "Storage",
    "SystemInfo",
    "Target",
    "Tethering",
    "Tracing",
    "WebAuthn",
];

/// Single commands that act on the whole profile, read local files, fetch
/// with the user's credentials outside the page, close or freeze the tab,
/// bypass a security check, or install state that outlives the grant.
const DENIED_METHODS: &[&str] = &[
    "DOM.setFileInputFiles",
    "Input.dispatchDragEvent",
    "Input.setIgnoreInputEvents",
    "Input.setInterceptDrags",
    "Memory.setPressureNotificationsSuppressed",
    "Memory.simulatePressureNotification",
    "Network.clearBrowserCache",
    "Network.clearBrowserCookies",
    "Network.emulateNetworkConditions",
    "Network.emulateNetworkConditionsByRule",
    "Network.enableReportingApi",
    "Network.getAllCookies",
    "Network.loadNetworkResource",
    "Network.overrideNetworkState",
    "Network.setAttachDebugStack",
    "Network.setBlockedURLs",
    "Network.setBypassServiceWorker",
    "Network.setCacheDisabled",
    "Network.setCookieControls",
    "Network.setExtraHTTPHeaders",
    "Network.setRequestInterception",
    "Network.setUserAgentOverride",
    "Page.addScriptToEvaluateOnLoad",
    "Page.addScriptToEvaluateOnNewDocument",
    "Page.captureSnapshot",
    "Page.close",
    "Page.crash",
    "Page.handleFileChooser",
    "Page.setBypassCSP",
    "Page.setDownloadBehavior",
    "Page.setInterceptFileChooserDialog",
    "Page.setRPHRegistrationMode",
    "Page.setSPCTransactionMode",
    "Page.setWebLifecycleState",
    "Runtime.addBinding",
    "Security.handleCertificateError",
    "Security.setIgnoreCertificateErrors",
    "Security.setOverrideCertificateErrors",
];

/// Parameter keys that name a storage partition; each must be the granted
/// origin's.
const ORIGIN_KEYS: &[&str] = &["securityOrigin", "storageKey"];

/// Parameter keys that would aim a command at another frame or execution
/// context (a same-site iframe can share the page's process), at a cache
/// named by another origin's storage key, or at local files. Commands run
/// on the main frame's default context only.
const DENIED_KEYS: &[&str] = &[
    "cacheId",
    "contextId",
    "executionContextId",
    "files",
    "frameId",
    "uniqueContextId",
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

/// Refuses a command that would leave the tab or the granted origin. The
/// caller has already checked the method's shape.
pub fn admit_command(
    method: &str,
    params: &Map<String, Value>,
    origin: &str,
) -> Result<(), BrowserFailure> {
    let domain = method.split_once('.').map_or(method, |(domain, _)| domain);
    if DENIED_DOMAINS.contains(&domain) || DENIED_METHODS.contains(&method) {
        return Err(BrowserFailure::Denied);
    }
    let params_value = Value::Object(params.clone());
    if !storage_keys_match(&params_value, origin) || !stays_in_main_frame(&params_value) {
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
            Some(Value::Array(cookies)) => cookies.iter().try_for_each(|cookie| {
                cookie
                    .as_object()
                    .map_or(Err(BrowserFailure::Denied), |c| cookie_target(c, origin))
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

fn storage_keys_match(value: &Value, origin: &str) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(key, value)| {
            if ORIGIN_KEYS.contains(&key.as_str()) {
                // A storage key may carry `^`-separated partition attributes.
                value.as_str().is_some_and(|text| {
                    same_origin(text.split('^').next().unwrap_or_default(), origin)
                })
            } else {
                storage_keys_match(value, origin)
            }
        }),
        Value::Array(items) => items.iter().all(|item| storage_keys_match(item, origin)),
        _ => true,
    }
}

/// No denied key at any depth, and no `pierce`, which would walk into
/// iframe documents.
fn stays_in_main_frame(value: &Value) -> bool {
    match value {
        Value::Object(map) => map.iter().all(|(key, value)| {
            !DENIED_KEYS.contains(&key.as_str())
                && !(key == "pierce" && value != &Value::Bool(false))
                && stays_in_main_frame(value)
        }),
        Value::Array(items) => items.iter().all(stays_in_main_frame),
        _ => true,
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
