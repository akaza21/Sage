use super::bounds::{admit_count, admit_screenshot, admit_text};
use super::redact::redact_result;
use sage_core::limits::BROWSER_LIMITS;
use serde::{Deserialize, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum BrowserFailure {
    InvalidRequest,
    Capacity,
    StaleHandle,
    Unavailable,
    OutcomeUnknown,
    Denied,
    NoAttachment,
    /// The user took over the tab; the Agent waits until they hand it back.
    UserControl,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserScope {
    pub session_id: String,
    pub tab_id: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserRequest {
    pub scope: BrowserScope,
    pub command: BrowserCommand,
}

impl BrowserRequest {
    pub fn act(scope: BrowserScope, action: BrowserAction) -> Self {
        Self {
            scope,
            command: BrowserCommand::Act { action },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserCommand {
    OpenTab {
        url: String,
    },
    Navigate {
        url: String,
    },
    SnapshotPage {},
    Act {
        action: BrowserAction,
    },
    WaitFor {
        condition: WaitCondition,
        #[serde(rename = "timeoutMs")]
        timeout_ms: u32,
    },
    Screenshot {
        region: Option<ViewportRegion>,
    },
    ReadConsole {
        cursor: Option<u64>,
        limit: usize,
    },
    ReadNetwork {
        cursor: Option<u64>,
        limit: usize,
    },
    /// Developer mode (ADR-0009b): one raw CDP command on the tab.
    Cdp {
        method: String,
        #[serde(default)]
        params: serde_json::Map<String, serde_json::Value>,
    },
}

impl BrowserCommand {
    /// Whether this command needs a developer-mode grant rather than an
    /// ordinary browser permission.
    pub fn is_elevated(&self) -> bool {
        matches!(self, Self::Cdp { .. })
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ElementHandle(String);

impl ElementHandle {
    pub fn from_token(token: String) -> Self {
        Self(token)
    }

    pub(crate) fn token(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for ElementHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ElementHandle([opaque])")
    }
}

#[derive(Clone, PartialEq, Eq, Deserialize)]
#[serde(transparent)]
pub struct FillSecret(String);

impl FillSecret {
    pub fn new(value: String) -> Self {
        Self(value)
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for FillSecret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("FillSecret([redacted])")
    }
}

impl Serialize for FillSecret {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str("[redacted]")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserAction {
    Click {
        handle: ElementHandle,
    },
    Fill {
        handle: ElementHandle,
        value: FillSecret,
    },
    Key {
        handle: ElementHandle,
        key: String,
    },
    Scroll {
        handle: ElementHandle,
        #[serde(rename = "deltaX")]
        delta_x: i32,
        #[serde(rename = "deltaY")]
        delta_y: i32,
    },
}

impl BrowserAction {
    pub fn handle(&self) -> &ElementHandle {
        match self {
            Self::Click { handle }
            | Self::Fill { handle, .. }
            | Self::Key { handle, .. }
            | Self::Scroll { handle, .. } => handle,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum WaitCondition {
    Handle { handle: ElementHandle },
    Text { text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ViewportRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SnapshotNode {
    pub handle: ElementHandle,
    pub role: String,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConsoleEntry {
    pub cursor: u64,
    pub level: String,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NetworkEntry {
    pub cursor: u64,
    pub method: String,
    pub url: String,
    pub status: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum BrowserResult {
    OpenTab {
        #[serde(rename = "tabId")]
        tab_id: u64,
    },
    Navigate {
        url: String,
        navigation_epoch: u64,
    },
    SnapshotPage {
        navigation_epoch: u64,
        text: String,
        nodes: Vec<SnapshotNode>,
    },
    Act {
        applied: bool,
    },
    WaitFor {
        matched: bool,
    },
    Screenshot {
        width: u32,
        height: u32,
        jpeg_base64: String,
    },
    ReadConsole {
        next_cursor: u64,
        entries: Vec<ConsoleEntry>,
    },
    ReadNetwork {
        next_cursor: u64,
        entries: Vec<NetworkEntry>,
    },
    Cdp {
        result: serde_json::Value,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BrowserResponse {
    pub scope: BrowserScope,
    pub result: BrowserResult,
}

pub trait BrowserCapability {
    fn attached_tab_id(&self) -> Option<u64> {
        None
    }
    fn perform(&mut self, request: &BrowserRequest) -> Result<BrowserResult, BrowserFailure>;
    /// Told when a request fails validation and is never performed, so a
    /// refused developer-mode call is still audited (ADR-0009b).
    fn refused(&mut self, _request: &BrowserRequest, _failure: BrowserFailure) {}
}

pub fn admit_request_json(bytes: &[u8]) -> Result<BrowserRequest, BrowserFailure> {
    admit_count(bytes.len(), BROWSER_LIMITS.request_bytes)?;
    let request: BrowserRequest =
        serde_json::from_slice(bytes).map_err(|_| BrowserFailure::InvalidRequest)?;
    validate_request(&request)?;
    Ok(request)
}

pub fn dispatch_json<P: BrowserCapability>(
    port: &mut P,
    bytes: &[u8],
) -> Result<BrowserResponse, BrowserFailure> {
    admit_count(bytes.len(), BROWSER_LIMITS.request_bytes)?;
    let request: BrowserRequest =
        serde_json::from_slice(bytes).map_err(|_| BrowserFailure::InvalidRequest)?;
    dispatch(port, request)
}

pub fn dispatch<P: BrowserCapability>(
    port: &mut P,
    request: BrowserRequest,
) -> Result<BrowserResponse, BrowserFailure> {
    if let Err(failure) = validate_request(&request) {
        port.refused(&request, failure);
        return Err(failure);
    }
    let mut result = port.perform(&request)?;
    redact_result(&mut result);
    validate_result(&result)?;
    let response = BrowserResponse {
        scope: request.scope,
        result,
    };
    let encoded = serde_json::to_vec(&response).map_err(|_| BrowserFailure::Unavailable)?;
    admit_count(encoded.len(), BROWSER_LIMITS.result_bytes)?;
    Ok(response)
}

pub fn validate_request(request: &BrowserRequest) -> Result<(), BrowserFailure> {
    admit_text("sessionId", &request.scope.session_id)?;
    if request.scope.session_id.is_empty()
        || (request.scope.tab_id == 0) != matches!(request.command, BrowserCommand::OpenTab { .. })
    {
        return Err(BrowserFailure::InvalidRequest);
    }
    match &request.command {
        BrowserCommand::OpenTab { url } | BrowserCommand::Navigate { url } => {
            admit_text("url", url)?;
            if url.chars().any(char::is_control) {
                return Err(BrowserFailure::InvalidRequest);
            }
            let parsed = url::Url::parse(url).map_err(|_| BrowserFailure::InvalidRequest)?;
            if !matches!(parsed.scheme(), "http" | "https")
                || parsed.username() != ""
                || parsed.password().is_some()
            {
                return Err(BrowserFailure::InvalidRequest);
            }
        }
        BrowserCommand::SnapshotPage {} => {}
        BrowserCommand::Act { action } => {
            admit_text("handle", action.handle().token())?;
            if action.handle().token().is_empty() {
                return Err(BrowserFailure::InvalidRequest);
            }
            match action {
                BrowserAction::Fill { value, .. } => admit_text("fill", value.expose())?,
                BrowserAction::Key { key, .. } => admit_text("key", key)?,
                BrowserAction::Scroll {
                    delta_x, delta_y, ..
                } => {
                    if delta_x.unsigned_abs() > BROWSER_LIMITS.scroll_delta as u32
                        || delta_y.unsigned_abs() > BROWSER_LIMITS.scroll_delta as u32
                    {
                        return Err(BrowserFailure::Capacity);
                    }
                }
                BrowserAction::Click { .. } => {}
            }
        }
        BrowserCommand::WaitFor {
            condition,
            timeout_ms,
        } => {
            if *timeout_ms == 0 || *timeout_ms > BROWSER_LIMITS.wait_timeout_ms {
                return Err(BrowserFailure::Capacity);
            }
            match condition {
                WaitCondition::Handle { handle } => admit_text("handle", handle.token())?,
                WaitCondition::Text { text } => admit_text("waitText", text)?,
            }
        }
        BrowserCommand::Screenshot { region } => {
            if let Some(region) = region {
                admit_screenshot(region.width, region.height, 0)?;
                if region.x.checked_add(region.width).is_none()
                    || region.y.checked_add(region.height).is_none()
                {
                    return Err(BrowserFailure::InvalidRequest);
                }
            }
        }
        BrowserCommand::ReadConsole { limit, .. } | BrowserCommand::ReadNetwork { limit, .. } => {
            if *limit == 0 {
                return Err(BrowserFailure::InvalidRequest);
            }
            admit_count(*limit, BROWSER_LIMITS.diagnostic_entries)?;
        }
        BrowserCommand::Cdp { method, params } => {
            super::developer::admit_method_shape(method)?;
            let encoded = serde_json::to_vec(params).map_err(|_| BrowserFailure::InvalidRequest)?;
            admit_count(encoded.len(), BROWSER_LIMITS.cdp_params_bytes)?;
        }
    }
    Ok(())
}

pub fn validate_result(result: &BrowserResult) -> Result<(), BrowserFailure> {
    match result {
        BrowserResult::OpenTab { tab_id } if *tab_id == 0 => {
            return Err(BrowserFailure::InvalidRequest);
        }
        BrowserResult::OpenTab { .. } => {}
        BrowserResult::Navigate { url, .. } => admit_text("url", url)?,
        BrowserResult::SnapshotPage { text, nodes, .. } => {
            admit_text("snapshotText", text)?;
            admit_count(nodes.len(), BROWSER_LIMITS.snapshot_nodes)?;
            for node in nodes {
                admit_text("handle", node.handle.token())?;
                admit_text("nodeRole", &node.role)?;
                admit_text("nodeName", &node.name)?;
            }
        }
        BrowserResult::Act { .. } | BrowserResult::WaitFor { .. } => {}
        BrowserResult::Screenshot {
            width,
            height,
            jpeg_base64,
        } => {
            admit_screenshot(*width, *height, jpeg_base64.len())?;
        }
        BrowserResult::ReadConsole { entries, .. } => {
            admit_count(entries.len(), BROWSER_LIMITS.diagnostic_entries)?;
            for entry in entries {
                admit_text("diagnosticLevel", &entry.level)?;
                admit_text("diagnosticText", &entry.text)?;
            }
        }
        BrowserResult::ReadNetwork { entries, .. } => {
            admit_count(entries.len(), BROWSER_LIMITS.diagnostic_entries)?;
            for entry in entries {
                admit_text("key", &entry.method)?;
                admit_text("url", &entry.url)?;
            }
        }
        BrowserResult::Cdp { result } => {
            let encoded = serde_json::to_vec(result).map_err(|_| BrowserFailure::Unavailable)?;
            admit_count(encoded.len(), BROWSER_LIMITS.cdp_result_bytes)?;
        }
    }
    let encoded = serde_json::to_vec(result).map_err(|_| BrowserFailure::Unavailable)?;
    admit_count(encoded.len(), BROWSER_LIMITS.result_bytes)
}
