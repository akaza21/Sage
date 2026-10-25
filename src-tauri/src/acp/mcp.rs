use serde_json::{Value, json};
use std::fmt;

const MAX_SERVERS: usize = 16;
const MAX_NAME: usize = 128;
const MAX_COMMAND: usize = 16_384;
const MAX_ARGS: usize = 64;
const MAX_ARG: usize = 4_096;
const MAX_ENV: usize = 32;
const MAX_ENV_NAME: usize = 128;
const MAX_ENV_VALUE: usize = 4_096;
const MAX_URL: usize = 2_048;

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct McpEnv {
    name: String,
    value: String,
}

impl fmt::Debug for McpEnv {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpEnv")
            .field("name", &self.name)
            .field("value", &"[redacted]")
            .finish()
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) enum McpServerDescriptor {
    Stdio {
        name: String,
        command: String,
        args: Vec<String>,
        env: Vec<McpEnv>,
    },
    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
    )]
    BrowserStdio(BrowserStdio),
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "non-empty MCP admission awaits coordinated durable registry migration"
        )
    )]
    Http { name: String, url: String },
    #[cfg_attr(
        not(test),
        expect(
            dead_code,
            reason = "non-empty MCP admission awaits coordinated durable registry migration"
        )
    )]
    Sse { name: String, url: String },
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct BrowserStdio {
    command: String,
    args: Vec<String>,
    env: Vec<McpEnv>,
}

impl fmt::Debug for McpServerDescriptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (transport, name) = match self {
            Self::Stdio { name, .. } => ("stdio", name.as_str()),
            Self::BrowserStdio(..) => ("stdio", "sage-browser"),
            Self::Http { name, .. } => ("http", name.as_str()),
            Self::Sse { name, .. } => ("sse", name.as_str()),
        };
        f.debug_struct("McpServerDescriptor")
            .field("transport", &transport)
            .field("name", &name)
            .finish()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum McpDescriptorError {
    Invalid,
    Capacity,
}

impl McpEnv {
    #[cfg(test)]
    pub(crate) fn for_test(name: &str, value: &str) -> Self {
        Self {
            name: name.into(),
            value: value.into(),
        }
    }
}

impl McpServerDescriptor {
    #[cfg(test)]
    pub(crate) fn stdio_for_test(
        name: &str,
        command: &str,
        args: &[&str],
        env: Vec<McpEnv>,
    ) -> Self {
        Self::Stdio {
            name: name.into(),
            command: command.into(),
            args: args.iter().map(|arg| (*arg).into()).collect(),
            env,
        }
    }

    #[cfg(test)]
    pub(crate) fn http_for_test(name: &str, url: &str) -> Self {
        Self::Http {
            name: name.into(),
            url: url.into(),
        }
    }

    #[cfg(test)]
    pub(crate) fn sse_for_test(name: &str, url: &str) -> Self {
        Self::Sse {
            name: name.into(),
            url: url.into(),
        }
    }

    fn name(&self) -> &str {
        match self {
            Self::Stdio { name, .. } | Self::Http { name, .. } | Self::Sse { name, .. } => name,
            Self::BrowserStdio(..) => "sage-browser",
        }
    }

    fn validate(&self) -> Result<(), McpDescriptorError> {
        if !valid_text(self.name(), MAX_NAME) {
            return Err(McpDescriptorError::Invalid);
        }
        match self {
            Self::Stdio {
                command, args, env, ..
            } => {
                if !valid_text(command, MAX_COMMAND) || !absolute_path(command) {
                    return Err(McpDescriptorError::Invalid);
                }
                if args.len() > MAX_ARGS || env.len() > MAX_ENV {
                    return Err(McpDescriptorError::Capacity);
                }
                if args.iter().any(|arg| !valid_text(arg, MAX_ARG))
                    || env.iter().any(|item| {
                        !valid_env_name(&item.name) || !valid_text(&item.value, MAX_ENV_VALUE)
                    })
                {
                    return Err(McpDescriptorError::Invalid);
                }
            }
            Self::BrowserStdio(browser) => {
                McpServerDescriptor::Stdio {
                    name: "sage-browser".into(),
                    command: browser.command.clone(),
                    args: browser.args.clone(),
                    env: browser.env.clone(),
                }
                .validate()?;
            }
            Self::Http { url, .. } | Self::Sse { url, .. } => {
                if !valid_text(url, MAX_URL) {
                    return Err(McpDescriptorError::Invalid);
                }
                let parsed = url::Url::parse(url).map_err(|_| McpDescriptorError::Invalid)?;
                if !matches!(parsed.scheme(), "http" | "https")
                    || !parsed.username().is_empty()
                    || parsed.password().is_some()
                    || parsed.fragment().is_some()
                {
                    return Err(McpDescriptorError::Invalid);
                }
            }
        }
        Ok(())
    }

    fn wire(&self) -> Value {
        match self {
            Self::Stdio {
                name,
                command,
                args,
                env,
            } => json!({
                "name": name,
                "command": command,
                "args": args,
                "env": env.iter().map(|item| json!({"name":item.name,"value":item.value})).collect::<Vec<_>>(),
            }),
            Self::BrowserStdio(browser) => json!({
                "name": "sage-browser",
                "command": browser.command,
                "args": browser.args,
                "env": browser.env.iter().map(|item| json!({"name":item.name,"value":item.value})).collect::<Vec<_>>(),
            }),
            Self::Http { name, url } => json!({"type":"http","name":name,"url":url,"headers":[]}),
            Self::Sse { name, url } => json!({"type":"sse","name":name,"url":url,"headers":[]}),
        }
    }
}

fn valid_text(value: &str, max: usize) -> bool {
    !value.is_empty() && value.len() <= max && !value.chars().any(char::is_control)
}

fn valid_env_name(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_ENV_NAME
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
        && !value.as_bytes()[0].is_ascii_digit()
}

fn absolute_path(value: &str) -> bool {
    let bytes = value.as_bytes();
    value.starts_with('/')
        || value.starts_with("\\\\")
        || (bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && matches!(bytes[2], b'\\' | b'/'))
}

#[derive(Clone)]
pub(crate) struct McpAdmission {
    servers: Vec<McpServerDescriptor>,
}

impl fmt::Debug for McpAdmission {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("McpAdmission")
            .field("server_count", &self.servers.len())
            .finish()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct McpCapabilities {
    pub(crate) http: bool,
    pub(crate) sse: bool,
}

impl McpCapabilities {
    pub(crate) fn from_initialize(response: &Value) -> Self {
        let value = &response["result"]["agentCapabilities"]["mcpCapabilities"];
        Self {
            http: value["http"].as_bool() == Some(true),
            sse: value["sse"].as_bool() == Some(true),
        }
    }
}

impl McpAdmission {
    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn is_empty(&self) -> bool {
        self.servers.is_empty()
    }

    pub(crate) fn browser_failure_detail(&self, error: &str) -> String {
        let Some(token) = self.servers.iter().find_map(|server| match server {
            McpServerDescriptor::BrowserStdio(browser) => browser
                .env
                .iter()
                .find(|item| item.name == "SAGE_BROWSER_MCP_TOKEN")
                .map(|item| item.value.as_str()),
            _ => None,
        }) else {
            return "browser descriptor unavailable; raw detail withheld".to_owned();
        };
        if token.len() != 64 || !token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return "browser startup detail withheld; invalid token descriptor".to_owned();
        }
        let redacted = error.replace(token, "[redacted]");
        if token.as_bytes().windows(8).any(|fragment| {
            redacted
                .as_bytes()
                .windows(8)
                .any(|candidate| candidate == fragment)
        }) {
            return "browser startup detail withheld; possible token fragment".to_owned();
        }
        let sanitized = crate::acp::redaction::sanitize_diagnostic_text(&redacted);
        if sanitized.len() <= 512 {
            return sanitized;
        }
        let mut end = 509;
        while !sanitized.is_char_boundary(end) {
            end -= 1;
        }
        format!("{}…", &sanitized[..end])
    }

    pub(crate) fn codex_config(&self) -> Result<Option<Value>, McpDescriptorError> {
        if self.servers.is_empty() {
            return Ok(None);
        }
        let mut servers = serde_json::Map::new();
        let mut shell_exclusions = Vec::new();
        for server in &self.servers {
            let (name, command, args, env, browser) = match server {
                McpServerDescriptor::Stdio {
                    name,
                    command,
                    args,
                    env,
                } => (name.as_str(), command, args, env, false),
                McpServerDescriptor::BrowserStdio(browser) => (
                    "sage-browser",
                    &browser.command,
                    &browser.args,
                    &browser.env,
                    true,
                ),
                _ => return Err(McpDescriptorError::Invalid),
            };
            if name == "sage-browser" && !browser {
                return Err(McpDescriptorError::Invalid);
            }
            if servers.contains_key(name) {
                return Err(McpDescriptorError::Invalid);
            }
            let mut config = json!({
                "command": command,
                "args": args,
                "env_vars": env.iter().map(|item| item.name.as_str()).collect::<Vec<_>>(),
                "required": false
            });
            if browser {
                config["default_tools_approval_mode"] = json!("approve");
                config["startup_timeout_sec"] = json!(30);
            }
            servers.insert(name.to_owned(), config);
            shell_exclusions.extend(env.iter().map(|item| item.name.as_str()));
        }
        Ok(Some(json!({
            "mcp_servers": servers,
            "shell_environment_policy": {"exclude": shell_exclusions}
        })))
    }

    pub(crate) fn codex_environment(&self) -> Result<Vec<(String, String)>, McpDescriptorError> {
        let mut result = Vec::new();
        for server in &self.servers {
            let env = match server {
                McpServerDescriptor::Stdio { env, .. } => env,
                McpServerDescriptor::BrowserStdio(browser) => &browser.env,
                _ => return Err(McpDescriptorError::Invalid),
            };
            for item in env {
                if result
                    .iter()
                    .any(|(name, _): &(String, String)| name.eq_ignore_ascii_case(&item.name))
                {
                    return Err(McpDescriptorError::Invalid);
                }
                result.push((item.name.clone(), item.value.clone()));
            }
        }
        Ok(result)
    }

    #[cfg_attr(
        not(windows),
        allow(dead_code, reason = "the in-app browser runtime is WebView2-only")
    )]
    pub(crate) fn browser_stdio(
        command: String,
        environment: [(String, String); 4],
    ) -> Result<Self, McpDescriptorError> {
        Self::from_trusted_descriptors(vec![McpServerDescriptor::BrowserStdio(BrowserStdio {
            command,
            args: vec![crate::acp::browser::mcp::transport::HELPER_ARGUMENT.into()],
            env: environment
                .into_iter()
                .map(|(name, value)| McpEnv { name, value })
                .collect(),
        })])
    }

    pub(crate) fn from_trusted_descriptors(
        servers: Vec<McpServerDescriptor>,
    ) -> Result<Self, McpDescriptorError> {
        if servers.len() > MAX_SERVERS {
            return Err(McpDescriptorError::Capacity);
        }
        for server in &servers {
            server.validate()?;
            if matches!(server, McpServerDescriptor::Stdio { name, .. } if name == "sage-browser") {
                return Err(McpDescriptorError::Invalid);
            }
        }
        Ok(Self { servers })
    }

    #[cfg(test)]
    pub(crate) fn for_test(servers: Vec<McpServerDescriptor>) -> Result<Self, McpDescriptorError> {
        Self::from_trusted_descriptors(servers)
    }

    pub(crate) fn wire(&self, capabilities: McpCapabilities) -> Vec<Value> {
        self.servers
            .iter()
            .filter(|server| match server {
                McpServerDescriptor::Stdio { .. } => true,
                McpServerDescriptor::BrowserStdio(..) => true,
                McpServerDescriptor::Http { .. } => capabilities.http,
                McpServerDescriptor::Sse { .. } => capabilities.sse,
            })
            .map(McpServerDescriptor::wire)
            .collect()
    }
}

impl Default for McpAdmission {
    fn default() -> Self {
        Self::from_trusted_descriptors(Vec::new()).expect("empty MCP admission is valid")
    }
}

#[cfg(test)]
mod tests;
