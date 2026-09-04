use super::{
    super::installed_role::SignedInstalledRole,
    AdmittedCodexBinding, BINDING_DIGEST, BINDING_DIGEST_HEX, CodexAdmissionUnavailable,
    ObservedEntrypoint, observed_environment,
    verify::{
        entrypoint_identity, observed_binding_digest, observed_entrypoint_identity, sha256_hex,
    },
};

impl AdmittedCodexBinding {
    pub(super) fn admit_signed_installed_role(
        packet: super::MetadataValidatedPacket,
        role: SignedInstalledRole,
        reproduced: super::ReproducedPackagePrerequisite,
        authority: super::CodexAuthorityPrerequisite,
        environment: super::CodexLaunchEnvironment,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        let expected_artifact = packet
            .physical_artifact_claims()
            .next()
            .map(|(_, relative_path)| relative_path)
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
        if role.binding_id != packet.metadata().binding_id
            || role.artifact_relative_path != expected_artifact
        {
            return Err(CodexAdmissionUnavailable::IndependentAdmission);
        }
        let entrypoint = entrypoint_identity(&packet)?;
        let artifact_digest = packet.metadata().entrypoint.sha256.clone();
        let harness_version = packet.metadata().codex_runtime_version.clone();
        let binding = Self {
            packet,
            reproduced: Some(reproduced),
            authority: Some(authority),
            environment: Some(environment),
            entrypoint,
            observed: None,
            binding_digest: BINDING_DIGEST,
            binding_digest_hex: BINDING_DIGEST_HEX.into(),
            artifact_digest,
            harness_version,
            #[cfg(all(test, windows))]
            journey_test_entrypoint: None,
            #[cfg(all(test, windows))]
            journey_test_cases: None,
            #[cfg(all(test, windows))]
            _cleanup: None,
        };
        binding.revalidate()?;
        Ok(binding)
    }

    pub(crate) fn admit_observed(
        executable: std::path::PathBuf,
        version: String,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        if version.is_empty() || version.len() > 512 {
            return Err(CodexAdmissionUnavailable::IndependentAdmission);
        }
        let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
            "../../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
        let entrypoint = observed_entrypoint_identity(&executable)?;
        let binding_digest = observed_binding_digest(&entrypoint);
        let artifact_digest = sha256_hex(entrypoint.sha256);
        let binding_digest_hex = sha256_hex(binding_digest);
        let environment = observed_environment(&entrypoint.path)?;
        Ok(Self {
            packet,
            reproduced: None,
            authority: None,
            environment: None,
            entrypoint,
            observed: Some(ObservedEntrypoint { environment }),
            binding_digest,
            binding_digest_hex,
            artifact_digest,
            harness_version: version,
            #[cfg(all(test, windows))]
            journey_test_entrypoint: None,
            #[cfg(all(test, windows))]
            journey_test_cases: None,
            #[cfg(all(test, windows))]
            _cleanup: None,
        })
    }
}
