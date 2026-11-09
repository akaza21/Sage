use super::{
    super::installed_role::SignedInstalledRole,
    AdmittedCodexBinding, BINDING_DIGEST, BINDING_DIGEST_HEX, CodexAdmissionUnavailable,
    ObservedEntrypoint, ObservedLaunchInputs, observed_environment,
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
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            desktop_fixture: None,
            binding_digest: BINDING_DIGEST,
            binding_digest_hex: BINDING_DIGEST_HEX.into(),
            artifact_digest,
            harness_version,
            characterisation:
                crate::acp::bindings::characterisation::Characterisation::Characterised,
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

    pub(crate) fn admit_observed_with_inputs(
        executable: std::path::PathBuf,
        version: String,
        inputs: std::sync::Arc<dyn ObservedLaunchInputs>,
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
        let environment = observed_environment(&entrypoint.path, inputs.as_ref())?;
        let characterisation =
            crate::acp::bindings::characterisation::codex_characterisation(&version);
        Ok(Self {
            packet,
            reproduced: None,
            authority: None,
            environment: None,
            entrypoint,
            observed: Some(ObservedEntrypoint {
                environment,
                inputs,
            }),
            #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
            desktop_fixture: None,
            binding_digest,
            binding_digest_hex,
            artifact_digest,
            harness_version: version,
            characterisation,
            #[cfg(all(test, windows))]
            journey_test_entrypoint: None,
            #[cfg(all(test, windows))]
            journey_test_cases: None,
            #[cfg(all(test, windows))]
            _cleanup: None,
        })
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn admit_desktop_fixture(
        selection: crate::launch_profile::CodexFixtureSelection,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        use sha2::{Digest, Sha256};

        selection
            .revalidate()
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
        let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
            "../../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
        let entrypoint = super::EntrypointIdentity {
            path: selection.artifact_path.clone(),
            sha256: selection.artifact_sha256,
            byte_len: selection.artifact_bytes,
        };
        let mut digest = Sha256::new();
        digest.update(b"sage:cp14:codex-desktop-fixture:v1");
        digest.update(selection.profile_root.as_os_str().as_encoded_bytes());
        digest.update(selection.artifact_path.as_os_str().as_encoded_bytes());
        digest.update(selection.artifact_sha256);
        digest.update(selection.artifact_bytes.to_le_bytes());
        digest.update(selection.case.as_bytes());
        let binding_digest: [u8; 32] = digest.finalize().into();
        let artifact_digest = sha256_hex(entrypoint.sha256);
        let environment = [
            ("USERPROFILE", &selection.profile_root),
            ("APPDATA", &selection.durable_dir),
            ("LOCALAPPDATA", &selection.durable_dir),
            ("TEMP", &selection.durable_dir),
            ("TMP", &selection.durable_dir),
        ]
        .into_iter()
        .map(|(name, path)| {
            fixture_child_environment_path(path).map(|path| (name.to_owned(), path))
        })
        .collect::<Result<Vec<_>, _>>()?;
        Ok(Self {
            packet,
            reproduced: None,
            authority: None,
            environment: None,
            entrypoint,
            observed: None,
            desktop_fixture: Some(super::DesktopCodexFixture {
                selection,
                environment,
            }),
            binding_digest,
            binding_digest_hex: sha256_hex(binding_digest),
            artifact_digest,
            harness_version: super::super::manifest::PINNED_CODEX_VERSION.into(),
            characterisation:
                crate::acp::bindings::characterisation::Characterisation::Characterised,
            #[cfg(all(test, windows))]
            journey_test_entrypoint: None,
            #[cfg(all(test, windows))]
            journey_test_cases: None,
            #[cfg(all(test, windows))]
            _cleanup: None,
        })
    }
}

#[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
fn fixture_child_environment_path(
    path: &std::path::Path,
) -> Result<String, CodexAdmissionUnavailable> {
    let canonical = path
        .to_str()
        .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
    let child_path = canonical.strip_prefix(r"\\?\").unwrap_or(canonical);
    if !matches!(child_path.as_bytes(), [drive, b':', b'\\', ..] if drive.is_ascii_alphabetic()) {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    Ok(child_path.to_owned())
}
