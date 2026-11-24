use sage_domain::harness::{HarnessDefinition, HarnessTransport};

pub(super) fn annotate_binding(
    definition: &mut HarnessDefinition,
    record: &crate::acp::bindings::registry::BindingRecord,
) {
    definition.prompt_resources = record.delivers_prompt_files();
    match record.bound_build() {
        Some(bound) => {
            definition.bound_version = safe_display_version(&bound.reported_version);
            definition.characterisation = Some(bound.characterisation);
            definition.account_model_discovery = bound.account_model_discovery;
            definition.access_modes = crate::acp::bindings::access_policy::access_modes(
                record.access_policy_binding_id(),
                &bound.reported_version,
                bound.characterisation.runs_tested_features(),
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
        prompt_resources: false,
        access_modes: Vec::new(),
        version: None,
        version_checking: false,
        bound_version: None,
        characterisation: None,
        unavailable_reason: None,
        sign_in: None,
        brand: None,
        update: None,
        latest_version: None,
        runs_version: None,
        direct_from_version: None,
    };
    definition
        .validate_transport_contract()
        .expect("curated harness metadata must satisfy its transport contract");
    definition
}

/// Untrusted CLI version prose may contain paths or secrets. Project only a
/// bounded dotted numeric token, never the executable identity or raw output.
pub(super) fn safe_display_version(version: &str) -> Option<String> {
    version.split_ascii_whitespace().find_map(|token| {
        let token = token.strip_prefix('v').unwrap_or(token);
        let parts: Vec<_> = token.split('.').collect();
        let numeric = token.len() <= 32
            && (2..=4).contains(&parts.len())
            && parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            });
        if numeric {
            return Some(token.to_owned());
        }
        let (base, prerelease) = token.split_once('-')?;
        let base_parts: Vec<_> = base.split('.').collect();
        (token.len() <= 96
            && base_parts.len() == 3
            && base_parts.iter().all(|part| {
                !part.is_empty()
                    && part.len() <= 8
                    && part.bytes().all(|byte| byte.is_ascii_digit())
            })
            && !prerelease.is_empty()
            && prerelease.len() <= 64
            && prerelease
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-'))
        .then(|| token.to_owned())
    })
}

#[cfg(test)]
mod tests {
    use super::safe_display_version;

    #[test]
    fn harness_display_version_never_echoes_paths_or_control_characters() {
        assert_eq!(
            safe_display_version("codex-cli v1.2.3"),
            Some("1.2.3".into())
        );
        assert_eq!(
            safe_display_version("codex-cli 0.155.0-alpha.16.3"),
            Some("0.155.0-alpha.16.3".into())
        );
        for text in [
            "C:\\private\\1.2.exe",
            "/home/private/1.2",
            "https://host/1.2",
            "1.2\u{1b}",
            "1.123456789",
            "",
            "private label",
            "0.155.0-",
            "0.155.0-alpha/secret",
        ] {
            assert_eq!(safe_display_version(text), None, "{text}");
        }
    }
}
