use std::sync::Arc;

use crate::acp::{
    bindings::{
        codex::{AdmittedCodexBinding, CODEX_BINDING_ID, CodexRuntimeFactory},
        runtime::RuntimeFactory,
    },
    intent::CapabilitySnapshotV1,
};

use super::{AdmittedBinding, BindingProjectRoot, RegistryAdmissionUnavailable};

pub(super) fn launch_factory(
    binding: AdmittedBinding,
    root: BindingProjectRoot,
) -> Box<dyn RuntimeFactory> {
    let (AdmittedBinding::Codex(binding), BindingProjectRoot::Codex(root)) = (binding, root) else {
        unreachable!("curated Codex record receives only Codex admission and root authority")
    };
    Box::new(CodexRuntimeFactory::from_admitted_binding(binding, root))
}

pub(super) fn capabilities(
    binding: &AdmittedBinding,
) -> Result<CapabilitySnapshotV1, RegistryAdmissionUnavailable> {
    let AdmittedBinding::Codex(binding) = binding else {
        return Err(RegistryAdmissionUnavailable::Drift);
    };
    binding
        .capability_ceiling()
        .map_err(RegistryAdmissionUnavailable::Codex)
}

pub(super) fn binding_id(binding: &AdmittedCodexBinding) -> &'static str {
    debug_assert_eq!(binding.binding_id(), CODEX_BINDING_ID);
    CODEX_BINDING_ID
}

pub(super) fn binding(binding: &AdmittedBinding) -> Option<&Arc<AdmittedCodexBinding>> {
    match binding {
        AdmittedBinding::Codex(binding) => Some(binding),
        AdmittedBinding::Claude(_) => None,
    }
}
