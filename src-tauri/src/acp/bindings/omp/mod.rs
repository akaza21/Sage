//! Oh My Pi v17.2.3 native ACP research binding.
//!
//! The pinned source exposes an out-of-process `omp acp` NDJSON surface. This
//! packet translates that surface without importing OMP's in-process SDK,
//! extensions, MCP providers, tools, or assets. It remains launch-ineligible
//! until the complete Bun/native-addon artifact graph and authority are
//! independently admitted.

mod manifest;
mod protocol;
mod runtime;
mod translator;
mod wire;

#[allow(unused_imports)]
pub(crate) use manifest::{
    ArtifactIdentity, CandidateManifest, CuratedOmpBinding, OMP_BUN_MINIMUM, OMP_LICENSE,
    OMP_PACKAGE, OMP_SOURCE_COMMIT, OMP_VERSION, OmpBindingCapabilities, OmpBindingError,
    OmpPlatform, ResearchBinding,
};
#[allow(unused_imports)]
pub(crate) use protocol::{
    AdapterEvent, ItemUpdateMode, ModelSummary, Operation, Outbound, PermissionResponse,
    PromptTerminal,
};
pub(crate) use runtime::OmpRuntime;
pub(crate) use translator::{OmpTranslator, ResourceMapper, TranslationError};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod snapshot_mapping_tests;
