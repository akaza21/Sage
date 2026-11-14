//! Everything is bounded and cleaned here, so the WebView receives only
//! short plain text, counts and closed sets of states. Authorization URLs
//! stay native.

use serde::Serialize;
use serde_json::{Map, Value};

#[path = "codex_page.rs"]
mod codex_page;

pub(crate) use codex_page::CodexMcpPage;
use codex_page::{CodexServerStatus, Text};

/// Additional servers set the truncated flag instead of being listed.
pub(crate) const MAX_LISTED_SERVERS: usize = 64;
const MAX_NAME_BYTES: usize = 128;
const MAX_TITLE_BYTES: usize = 256;
const MAX_ERROR_BYTES: usize = 512;
const MAX_ORIGIN_BYTES: usize = 256;
const MAX_AUTH_URL_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum McpConnection {
    NotStarted,
    Starting,
    Connected,
    SignInRequired,
    Failed,
    Cancelled,
    Disabled,
    /// The harness did not say, or the configuration changed since.
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum McpAuth {
    /// Signed in through OAuth.
    OAuth,
    /// A configured bearer token.
    Token,
    /// Supports OAuth and is not signed in.
    SignedOut,
    /// Needs no sign-in.
    None,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpServer {
    pub(crate) name: String,
    pub(crate) title: Option<String>,
    pub(crate) version: Option<String>,
    pub(crate) connection: McpConnection,
    pub(crate) auth: McpAuth,
    pub(crate) tools: u32,
    pub(crate) resources: u32,
    /// Why its tools could not be listed.
    pub(crate) error: Option<String>,
    /// The scheme, host and port of an HTTP server; none for a local one.
    pub(crate) origin: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpActions {
    pub(crate) reload: bool,
    /// Open a server's sign-in page, which Sage can do on this system.
    pub(crate) sign_in: bool,
    pub(crate) reconnect: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct McpInventory {
    pub(crate) servers: Vec<McpServer>,
    pub(crate) truncated: bool,
    pub(crate) actions: McpActions,
}

impl McpInventory {
    /// A Codex `mcpServerStatus/list` result. A server without a usable
    /// name is left out.
    pub(crate) fn from_codex(page: CodexMcpPage) -> Self {
        let more = page.has_more();
        let servers: Vec<McpServer> = page
            .data
            .into_iter()
            .filter_map(|server| server.0)
            .filter_map(codex_server)
            .collect();
        let actions = McpActions {
            reload: true,
            sign_in: crate::system_browser::available(),
            reconnect: false,
        };
        Self::listed(servers, more, actions)
    }

    /// A Claude Code `mcp_status` reply. Its server configuration (URLs,
    /// headers, OAuth settings) is never read past an HTTP origin.
    pub(crate) fn from_claude(reply: &Value) -> Option<Self> {
        let servers = reply
            .get("mcpServers")?
            .as_array()?
            .iter()
            .filter_map(Value::as_object)
            .filter_map(claude_server)
            .collect();
        let actions = McpActions {
            reload: false,
            sign_in: false,
            reconnect: true,
        };
        Some(Self::listed(servers, false, actions))
    }

    fn listed(mut servers: Vec<McpServer>, more: bool, actions: McpActions) -> Self {
        let truncated = more || servers.len() > MAX_LISTED_SERVERS;
        servers.sort_by(|left, right| left.name.cmp(&right.name));
        servers.truncate(MAX_LISTED_SERVERS);
        Self {
            servers,
            truncated,
            actions,
        }
    }
}

/// The name a harness knows the server by, shown as it is or not at all:
/// a cleaned name would act on no server.
fn exact_name(server: &Map<String, Value>) -> Option<String> {
    let name = server.get("name")?.as_str()?;
    is_exact_name(name).then(|| name.to_owned())
}

fn is_exact_name(name: &str) -> bool {
    clean(name, MAX_NAME_BYTES).as_deref() == Some(name)
}

fn claude_server(server: &Map<String, Value>) -> Option<McpServer> {
    let name = exact_name(server)?;
    let info = server.get("serverInfo").and_then(Value::as_object);
    let text = |key: &str, max| {
        info.and_then(|info| info.get(key))
            .and_then(Value::as_str)
            .and_then(|value| clean(value, max))
    };
    let status = server.get("status").and_then(Value::as_str);
    let config = server.get("config").and_then(Value::as_object);
    let remote = config
        .and_then(|config| config.get("type"))
        .and_then(Value::as_str)
        .is_some_and(|kind| matches!(kind, "http" | "sse"));
    Some(McpServer {
        title: text("name", MAX_TITLE_BYTES).filter(|title| *title != name),
        version: text("version", MAX_NAME_BYTES),
        connection: match status {
            Some("pending") => McpConnection::Starting,
            Some("connected") => McpConnection::Connected,
            Some("needs-auth") => McpConnection::SignInRequired,
            Some("failed") => McpConnection::Failed,
            Some("disabled") => McpConnection::Disabled,
            _ => McpConnection::Unknown,
        },
        auth: if status == Some("needs-auth") {
            McpAuth::SignedOut
        } else {
            McpAuth::Unknown
        },
        tools: server
            .get("tools")
            .and_then(Value::as_array)
            .map_or(0, |tools| u32::try_from(tools.len()).unwrap_or(u32::MAX)),
        resources: 0,
        error: server
            .get("error")
            .and_then(Value::as_str)
            .and_then(|error| clean(error, MAX_ERROR_BYTES)),
        origin: config
            .filter(|_| remote)
            .and_then(|config| config.get("url"))
            .and_then(Value::as_str)
            .and_then(origin),
        name,
    })
}

fn codex_server(server: CodexServerStatus) -> Option<McpServer> {
    let name = server.name.0.filter(|name| is_exact_name(name))?;
    let info = server.server_info.0.unwrap_or_default();
    let text = |value: Text, max| value.0.and_then(|value| clean(&value, max));
    Some(McpServer {
        title: text(info.title, MAX_TITLE_BYTES).filter(|title| *title != name),
        version: text(info.version, MAX_NAME_BYTES),
        connection: match server.runtime_status.0.as_deref() {
            Some("notStarted") => McpConnection::NotStarted,
            Some("starting") => McpConnection::Starting,
            Some("connected") => McpConnection::Connected,
            Some("authenticationRequired") => McpConnection::SignInRequired,
            Some("failed") => McpConnection::Failed,
            Some("cancelled") => McpConnection::Cancelled,
            Some("disabled") => McpConnection::Disabled,
            _ => McpConnection::Unknown,
        },
        auth: match server.auth_status.0.as_deref() {
            Some("oAuth") => McpAuth::OAuth,
            Some("bearerToken") => McpAuth::Token,
            Some("notLoggedIn") => McpAuth::SignedOut,
            Some("unsupported") => McpAuth::None,
            _ => McpAuth::Unknown,
        },
        tools: server.tools.0,
        resources: server.resources.0,
        error: text(server.tools_error, MAX_ERROR_BYTES),
        origin: server.http_origin.0.as_deref().and_then(origin),
        name,
    })
}

/// One line of plain text, clipped at a character boundary; `None` when
/// nothing printable is left.
fn clean(value: &str, max: usize) -> Option<String> {
    let mut text: String = value
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if text.len() > max {
        let mut end = max;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    (!text.is_empty()).then_some(text)
}

/// An http or https origin with no credentials, path, query or fragment.
fn origin(value: &str) -> Option<String> {
    if value.len() > MAX_ORIGIN_BYTES {
        return None;
    }
    let url = url::Url::parse(value).ok()?;
    let plain = matches!(url.scheme(), "http" | "https")
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none();
    plain.then(|| url.origin().ascii_serialization())
}

/// Where the user signs in to an MCP server. It is opened in the system
/// browser and never reaches the WebView or a log.
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct McpAuthUrl(String);

impl std::fmt::Debug for McpAuthUrl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("McpAuthUrl([redacted])")
    }
}

impl McpAuthUrl {
    /// An https URL with a host and no credentials or line breaks.
    pub(crate) fn parse(value: &str) -> Option<Self> {
        if value.is_empty()
            || value.len() > MAX_AUTH_URL_BYTES
            || value.chars().any(char::is_control)
        {
            return None;
        }
        let url = url::Url::parse(value).ok()?;
        let admitted = url.scheme() == "https"
            && url.host_str().is_some_and(|host| !host.is_empty())
            && url.username().is_empty()
            && url.password().is_none();
        let url: String = url.into();
        (admitted && url.len() <= MAX_AUTH_URL_BYTES).then_some(Self(url))
    }

    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
