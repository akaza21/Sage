use serde_json::{Map, Value};

use crate::acp::bindings::characterisation::valid_numeric_version;
use sage_acp_protocol::intent::CapabilitySnapshotV1;

use super::{AdapterError, CodexAdapter, NegotiatedMethods};
use crate::acp::bindings::codex::{degrade, request::APP_SERVER_CLIENT_IDENTITY};

const USER_AGENT_BYTES_MAX: usize = 1024;
const WINDOWS_VERSION_COMPONENTS_MAX: usize = 4;
const PRODUCT_TOKEN_BYTES_MAX: usize = 64;

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
        // The built-ins list as soon as the session can run them; the skills
        // follow their own read.
        self.list_commands(Vec::new());
        Ok(snapshot)
    }
}

/// Validates the `initialize` result fields Sage relies on and ignores the
/// rest, so a Codex release that adds fields or runs on another platform still
/// starts.
///
/// * `userAgent` must name the version the discovery probe saw, so the process
///   that answered is the build Sage admitted.
/// * `codexHome` must be absolute, and for the curated package it must be the
///   home Sage provisioned.
/// * The curated package is Windows-only, so it must report a Windows host.
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
    if !valid_user_agent(user_agent, runtime_version, identity) {
        return false;
    }
    match identity {
        InitializeIdentity::Observed => valid_observed_codex_home(observed_home),
        InitializeIdentity::Curated => {
            observed_home == codex_home
                && text("platformFamily") == Some("windows")
                && text("platformOs") == Some("windows")
        }
    }
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

/// Codex reports `<originator>/<version> <platform and client details>`. An
/// observed Codex is checked only for its originator and version, because the
/// platform and client details vary by host and release. The curated package
/// is one known x86_64 Windows build, so its whole user agent is checked,
/// including the echo of Sage's own client identity.
fn valid_user_agent(value: &str, runtime_version: &str, identity: InitializeIdentity) -> bool {
    if value.is_empty() || value.len() > USER_AGENT_BYTES_MAX || value.chars().any(char::is_control)
    {
        return false;
    }
    let runtime_version = runtime_version
        .split_ascii_whitespace()
        .last()
        .unwrap_or_default();
    if runtime_version.is_empty() {
        return false;
    }
    let Some((product, rest)) = value.split_once('/') else {
        return false;
    };
    let Some(details) = rest.strip_prefix(runtime_version) else {
        return false;
    };
    match identity {
        InitializeIdentity::Observed => {
            valid_product_token(product) && (details.is_empty() || details.starts_with(' '))
        }
        InitializeIdentity::Curated => {
            product == APP_SERVER_CLIENT_IDENTITY.name && valid_curated_details(details)
        }
    }
}

/// Matches ` (Windows <version>; x86_64) <terminal> (<client name>; <client version>)`.
fn valid_curated_details(details: &str) -> bool {
    let client = APP_SERVER_CLIENT_IDENTITY;
    let suffix = format!(" ({}; {})", client.name, client.version);
    let Some(platform_and_terminal) = details
        .strip_prefix(" (Windows ")
        .and_then(|body| body.strip_suffix(&suffix))
    else {
        return false;
    };
    let Some((windows_version, terminal)) = platform_and_terminal.split_once("; x86_64) ") else {
        return false;
    };
    valid_numeric_version(windows_version, 1, WINDOWS_VERSION_COMPONENTS_MAX)
        && !terminal.is_empty()
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
