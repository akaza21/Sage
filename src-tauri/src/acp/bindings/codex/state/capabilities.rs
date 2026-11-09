use serde_json::{Map, Value};

use crate::acp::bindings::characterisation::valid_numeric_version;
use crate::acp::intent::CapabilitySnapshotV1;

use super::{AdapterError, CodexAdapter, NegotiatedMethods};
use crate::acp::bindings::codex::{degrade, request::APP_SERVER_CLIENT_IDENTITY};

const INITIALIZE_RESULT_FIELDS: [&str; 4] =
    ["userAgent", "codexHome", "platformFamily", "platformOs"];
const USER_AGENT_BYTES_MAX: usize = 1024;
const PRODUCT_TOKEN_BYTES_MAX: usize = 64;
const WINDOWS_VERSION_COMPONENTS_MAX: usize = 4;

impl CodexAdapter {
    pub(crate) fn accept_initialize(
        &mut self,
        result: &Map<String, Value>,
    ) -> Result<CapabilitySnapshotV1, AdapterError> {
        let valid = valid_initialize_result(
            result,
            &self.runtime_version,
            &self.codex_home,
            if self.codex_home.is_empty() {
                InitializeIdentity::Observed
            } else {
                InitializeIdentity::Curated
            },
        );
        if !valid {
            if crate::acp::bindings::codex::incoming::reports_diagnostics() {
                tracing::warn!(
                    runtime_version = self.runtime_version,
                    observed = self.observed_build_characterisation.is_some(),
                    result_fields = ?result.keys().collect::<Vec<_>>(),
                    user_agent = ?result.get("userAgent"),
                    codex_home = ?result.get("codexHome"),
                    platform_family = ?result.get("platformFamily"),
                    platform_os = ?result.get("platformOs"),
                    "Codex discovery initialize identity validation failed"
                );
            }
            return Err(AdapterError::CapabilityUnavailable);
        }

        let observed = self
            .observed_build_characterisation
            .map(|characterisation| degrade::ObservedBuild {
                reported_version: &self.runtime_version,
                characterisation,
            });
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        let (snapshot, models) = if self.desktop_fixture {
            (
                degrade::projected_desktop_fixture_capabilities(&self.capability_ceiling)
                    .map_err(|_| AdapterError::CapabilityUnavailable)?,
                self.desktop_fixture_lists_models,
            )
        } else {
            degrade::projected_capabilities(&self.capability_ceiling, observed)
                .map_err(|_| AdapterError::CapabilityUnavailable)?
        };
        #[cfg(not(all(windows, feature = "acp-fixture", debug_assertions)))]
        let (snapshot, models) =
            degrade::projected_capabilities(&self.capability_ceiling, observed)
                .map_err(|_| AdapterError::CapabilityUnavailable)?;
        self.negotiated = Some(NegotiatedMethods {
            snapshot: snapshot.clone(),
            models,
        });
        Ok(snapshot)
    }
}

pub(crate) fn valid_initialize_result(
    result: &Map<String, Value>,
    runtime_version: &str,
    codex_home: &str,
    identity: InitializeIdentity,
) -> bool {
    let text = |name: &str| result.get(name).and_then(Value::as_str);
    let (Some(user_agent), Some(observed_home)) = (text("userAgent"), text("codexHome")) else {
        return false;
    };
    if matches!(identity, InitializeIdentity::Observed) {
        return result.len() == INITIALIZE_RESULT_FIELDS.len()
            && INITIALIZE_RESULT_FIELDS
                .iter()
                .all(|field| result.contains_key(*field))
            && valid_user_agent(user_agent, runtime_version, InitializeIdentity::Observed)
            && valid_observed_codex_home(observed_home)
            && valid_platform_identity(
                text("platformFamily").unwrap_or_default(),
                text("platformOs").unwrap_or_default(),
                InitializeIdentity::Observed,
            );
    }
    result.len() == INITIALIZE_RESULT_FIELDS.len()
        && INITIALIZE_RESULT_FIELDS
            .iter()
            .all(|field| result.contains_key(*field))
        && valid_user_agent(user_agent, runtime_version, InitializeIdentity::Curated)
        && observed_home == codex_home
        && valid_platform_identity(
            text("platformFamily").unwrap_or_default(),
            text("platformOs").unwrap_or_default(),
            InitializeIdentity::Curated,
        )
}

fn valid_observed_codex_home(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32 * 1024
        && !value.chars().any(char::is_control)
        && (value.starts_with('/')
            || value.as_bytes().get(0..3).is_some_and(|prefix| {
                prefix[0].is_ascii_alphabetic()
                    && prefix[1] == b':'
                    && matches!(prefix[2], b'\\' | b'/')
            }))
}

#[derive(Clone, Copy)]
pub(crate) enum InitializeIdentity {
    Curated,
    Observed,
}

fn valid_platform_identity(family: &str, os: &str, identity: InitializeIdentity) -> bool {
    if matches!(identity, InitializeIdentity::Curated) {
        return family == "windows" && os == "windows";
    }
    matches!(
        (family, os),
        ("windows", "windows") | ("macos", "macos") | ("macOS", "macOS") | ("unix", "linux")
    )
}

fn valid_product_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= PRODUCT_TOKEN_BYTES_MAX
        && !value.starts_with(' ')
        && !value.ends_with(' ')
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b' ' | b'.' | b'-' | b'_' | b'+')
        })
}

fn valid_user_agent(value: &str, runtime_version: &str, identity: InitializeIdentity) -> bool {
    if value.is_empty() || value.len() > USER_AGENT_BYTES_MAX || value.chars().any(char::is_control)
    {
        return false;
    }
    let client = APP_SERVER_CLIENT_IDENTITY;
    let runtime_version = runtime_version
        .split_ascii_whitespace()
        .last()
        .unwrap_or(runtime_version);
    let marker = format!("/{runtime_version} (Windows ");
    let suffix = format!(" ({}; {})", client.name, client.version);
    let Some(body) = value.strip_suffix(&suffix) else {
        return false;
    };
    let Some((product, middle)) = body.split_once(&marker) else {
        return false;
    };
    if !valid_product_token(product)
        || (matches!(identity, InitializeIdentity::Curated) && product != client.name)
    {
        return false;
    }
    let Some((windows_version, terminal)) = middle.split_once("; x86_64) ") else {
        return false;
    };
    valid_windows_version(windows_version) && !terminal.is_empty()
}

fn valid_windows_version(value: &str) -> bool {
    valid_numeric_version(value, 1, WINDOWS_VERSION_COMPONENTS_MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_windows_version_requires_nonempty_numeric_components() {
        let client = APP_SERVER_CLIENT_IDENTITY;
        for version in ["10..0", "...", ".10.0", "10.0."] {
            let pinned = crate::acp::bindings::codex::manifest::PINNED_CODEX_VERSION;
            let user_agent = format!(
                "Codex Desktop/{pinned} (Windows {version}; x86_64) dumb ({}; {})",
                client.name, client.version
            );
            assert!(!valid_user_agent(
                &user_agent,
                &format!("codex {pinned}"),
                InitializeIdentity::Observed,
            ));
        }
    }
}
