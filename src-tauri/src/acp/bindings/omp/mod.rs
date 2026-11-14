//! This OMP binding remains launch-ineligible until the complete Bun/native-addon artifact graph
//! and authority are independently admitted.

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
    AdapterEvent, ItemUpdateMode, ModelSummary, Operation, Outbound, PromptTerminal,
};
pub(crate) use runtime::OmpRuntime;
pub(crate) use translator::{OmpTranslator, ResourceMapper, TranslationError};

#[cfg(test)]
mod tests;

#[cfg(test)]
mod snapshot_mapping_tests;
