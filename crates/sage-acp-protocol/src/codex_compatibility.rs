//! Observed builds are admitted only by committed, schema-backed wire evidence.
//! Release ordering never grants protocol capabilities.

use std::sync::OnceLock;

use serde::Deserialize;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CodexCapabilities {
    pub session_extensions: bool,
    pub access_policy: bool,
    pub goals: bool,
    pub browser: bool,
}

impl CodexCapabilities {
    pub fn characterised(self) -> bool {
        self.session_extensions && self.access_policy && self.goals && self.browser
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ProvenBuild {
    version: String,
    capabilities: CodexCapabilities,
}

fn builds() -> &'static [ProvenBuild] {
    static BUILDS: OnceLock<Vec<ProvenBuild>> = OnceLock::new();
    BUILDS.get_or_init(|| {
        serde_json::from_str(include_str!("codex_proven_builds.json"))
            .expect("Codex evidence is validated by the wire compatibility tests")
    })
}

/// Only the exact release token backed by evidence is accepted, including when
/// supplied as `codex-cli <version>`. Prereleases and build suffixes need their
/// own evidence; malformed or unrecorded releases stay degraded.
pub fn observed_capabilities(reported_version: &str) -> CodexCapabilities {
    let Some(version) = reported_version.split_ascii_whitespace().next_back() else {
        return CodexCapabilities::default();
    };
    builds()
        .iter()
        .find(|build| build.version == version)
        .map(|build| build.capabilities)
        .unwrap_or_default()
}

pub fn protocol_capabilities(identity: &str) -> CodexCapabilities {
    if identity == super::codex_protocol::PINNED_PROTOCOL_IDENTITY {
        return CodexCapabilities {
            session_extensions: true,
            access_policy: true,
            goals: true,
            browser: true,
        };
    }
    identity
        .strip_prefix("codex-app-server-observed-")
        .filter(|version| !version.bytes().any(|byte| byte.is_ascii_whitespace()))
        .map(observed_capabilities)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod schema;
