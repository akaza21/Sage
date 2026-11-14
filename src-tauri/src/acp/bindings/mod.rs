// Adapter source packets compile against the shared contract before an exact
// external artifact is admitted. The dormant surface becomes live only through
// curated capability and artifact validation.
pub(crate) mod access_policy;
pub(crate) mod adapter;
pub(crate) mod characterisation;
pub(crate) mod claude;
#[allow(
    dead_code,
    unused_imports,
    reason = "covers the live Codex binding too: PROV-01 provisioning and Windows-only paths are staged inside it"
)]
pub(crate) mod codex;
pub(crate) mod command_summary;
pub(crate) mod node_engine;
#[allow(
    dead_code,
    unused_imports,
    reason = "ADE-0006E: the Oh My Pi binding is staged and not registered"
)]
pub(crate) mod omp;
#[allow(
    dead_code,
    unused_imports,
    reason = "ADE-0006C: the production Pi binding is staged; only the Windows debug fixture lane registers Pi"
)]
pub(crate) mod pi;
pub(crate) mod registry;
pub(crate) mod release_version;
pub(crate) mod runtime;
pub(crate) mod session_title;
pub(crate) mod standard_acp;
