use crate::acp_protocol::codex_protocol::{
    PINNED_CODEX_VERSION, pinned_codex_version, pinned_schema_prefix,
};

pub(crate) const MANAGED_CODEX_HOME: &str = concat!(
    r"C:\SageHarnesses\CodexState\codex-app-server-",
    pinned_codex_version!(),
    "-",
    pinned_schema_prefix!()
);

pub(crate) fn version_line(program: &str) -> String {
    format!("{program} {PINNED_CODEX_VERSION}")
}

pub(crate) fn user_agent(product: &str, terminal: &str) -> String {
    format!(
        "{product}/{PINNED_CODEX_VERSION} (Windows 10.0.26200; x86_64) {terminal} (sage; 0.1.0)"
    )
}
