//! Wire identities a pinned Codex app-server reports, for tests that replay
//! its `initialize` handshake. Everything is derived from the manifest pin.

use super::manifest::{PINNED_CODEX_VERSION, pinned_codex_version, pinned_schema_prefix};

/// The isolated `CODEX_HOME` of the curated packet under the default root.
pub(crate) const MANAGED_CODEX_HOME: &str = concat!(
    r"C:\SageHarnesses\CodexState\codex-app-server-",
    pinned_codex_version!(),
    "-",
    pinned_schema_prefix!()
);

/// The `--version` line `program` prints for the pinned release, such as
/// `codex-cli 0.167.0`.
pub(crate) fn version_line(program: &str) -> String {
    format!("{program} {PINNED_CODEX_VERSION}")
}

/// The `userAgent` the pinned app-server reports to the Sage client for an
/// originator `product` and a `terminal` token.
pub(crate) fn user_agent(product: &str, terminal: &str) -> String {
    format!(
        "{product}/{PINNED_CODEX_VERSION} (Windows 10.0.26200; x86_64) {terminal} (sage; 0.1.0)"
    )
}
