use crate::domain::{HarnessDefinition, HarnessTransport};

pub(super) fn annotate_binding(
    definition: &mut HarnessDefinition,
    record: &crate::acp::bindings::registry::BindingRecord,
) {
    match record.bound_build() {
        Some(bound) => {
            definition.bound_version = super::safe_display_version(&bound.reported_version);
            definition.characterisation = Some(bound.characterisation);
            definition.account_model_discovery = bound.account_model_discovery;
            definition.access_modes = crate::acp::bindings::access_policy::access_modes(
                record.access_policy_binding_id(),
                &bound.reported_version,
                bound.characterisation.is_characterised(),
            );
        }
        None => definition.unavailable_reason = record.unavailable_reason(),
    }
}

pub(super) struct CuratedProtocol<'a> {
    transport: HarnessTransport,
    binding_id: Option<&'a str>,
    account_model_discovery: bool,
}

impl<'a> CuratedProtocol<'a> {
    pub(super) fn account_models(binding_id: &'a str) -> Self {
        Self {
            transport: HarnessTransport::Acp,
            binding_id: Some(binding_id),
            account_model_discovery: true,
        }
    }

    pub(super) fn acp(binding_id: Option<&'a str>) -> Self {
        Self {
            transport: HarnessTransport::Acp,
            binding_id,
            account_model_discovery: false,
        }
    }

    pub(super) fn legacy_simulator() -> Self {
        Self {
            transport: HarnessTransport::LegacySimulator,
            binding_id: None,
            account_model_discovery: false,
        }
    }
}

pub(super) fn definition(
    id: &str,
    name: &str,
    description: &str,
    install_state: &str,
    source: &str,
    protocol: CuratedProtocol<'_>,
) -> HarnessDefinition {
    let definition = HarnessDefinition {
        id: id.into(),
        name: name.into(),
        description: description.into(),
        install_state: install_state.into(),
        source: source.into(),
        transport: protocol.transport,
        binding_id: protocol.binding_id.map(str::to_owned),
        account_model_discovery: protocol.account_model_discovery,
        access_modes: Vec::new(),
        version: None,
        version_checking: false,
        bound_version: None,
        characterisation: None,
        unavailable_reason: None,
        sign_in: None,
        brand: None,
    };
    definition
        .validate_transport_contract()
        .expect("curated harness metadata must satisfy its transport contract");
    definition
}
