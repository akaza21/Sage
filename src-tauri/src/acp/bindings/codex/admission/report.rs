use super::{AdmittedCodexBinding, BINDING_ID, CodexLaunchEnvironment, EntrypointIdentity};

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexAdmissionUnavailable {
    #[error("Codex Harness Admission is unavailable on this platform")]
    Platform,
    #[error("the workstation can only observe the Codex Authenticode role")]
    ObservationOnly,
    #[error("the exact Codex Binding has not completed independent Harness Admission")]
    IndependentAdmission,
    #[error("the admitted Codex Binding changed after admission")]
    Drift,
    #[error(transparent)]
    Projection(#[from] super::super::degrade::ProjectionUnavailable),
}

impl AdmittedCodexBinding {
    pub(crate) fn binding_id(&self) -> &str {
        BINDING_ID
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        self.binding_digest
    }

    pub(crate) fn binding_digest_hex(&self) -> &str {
        &self.binding_digest_hex
    }

    pub(crate) fn artifact_digest(&self) -> &str {
        &self.artifact_digest
    }

    pub(crate) fn harness_version(&self) -> &str {
        &self.harness_version
    }

    pub(crate) fn is_observed(&self) -> bool {
        self.observed.is_some()
    }

    pub(crate) fn is_desktop_fixture(&self) -> bool {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        return self.desktop_fixture.is_some();
        #[cfg(not(all(windows, feature = "acp-fixture", debug_assertions)))]
        false
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn desktop_fixture_case(&self) -> Option<&str> {
        self.desktop_fixture
            .as_ref()
            .map(|fixture| fixture.selection.case.as_str())
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn desktop_fixture_project_root(&self) -> Option<&std::path::Path> {
        self.desktop_fixture
            .as_ref()
            .map(|fixture| fixture.selection.project_root.as_path())
    }

    #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
    pub(crate) fn consume_desktop_fixture(&self) -> Result<(), CodexAdmissionUnavailable> {
        self.desktop_fixture
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?
            .selection
            .consume_one_shot()
            .map_err(|_| CodexAdmissionUnavailable::Drift)
    }

    pub(crate) fn characterisation(
        &self,
    ) -> crate::acp::bindings::characterisation::Characterisation {
        self.characterisation
    }

    pub(super) fn observed_build(&self) -> Option<super::super::degrade::ObservedBuild<'_>> {
        self.observed
            .is_some()
            .then(|| super::super::degrade::ObservedBuild {
                reported_version: &self.harness_version,
                characterisation: self.characterisation,
            })
    }

    pub(crate) fn entrypoint(&self) -> &EntrypointIdentity {
        &self.entrypoint
    }

    #[cfg(windows)]
    pub(crate) fn package_manifest(&self) -> Option<&crate::acp::process::ManifestExpectation> {
        self.observed.as_ref()?.manifest.as_ref()
    }

    /// Whether `other` admitted the same installed build: the same launcher,
    /// reported version and, where launch reads it, package manifest. The
    /// manifest is compared here rather than folded into the binding digest,
    /// so chats persisted against an unchanged installation keep resuming.
    pub(crate) fn is_same_installation(&self, other: &Self) -> bool {
        #[cfg(windows)]
        let same_manifest = self.package_manifest() == other.package_manifest();
        #[cfg(not(windows))]
        let same_manifest = true;
        same_manifest
            && self.artifact_digest() == other.artifact_digest()
            && self.harness_version() == other.harness_version()
    }

    #[cfg(all(test, windows))]
    pub(crate) fn journey_test_entrypoint(&self) -> Option<&EntrypointIdentity> {
        self.journey_test_entrypoint.as_ref()
    }

    #[cfg(all(test, windows))]
    pub(crate) fn next_journey_test_case(&self) -> Option<&'static str> {
        self.journey_test_cases
            .as_ref()?
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .pop_front()
    }

    pub(crate) fn packet(&self) -> &super::MetadataValidatedPacket {
        &self.packet
    }

    pub(crate) fn environment(&self) -> &CodexLaunchEnvironment {
        self.environment
            .as_ref()
            .expect("managed environment is not available for observed Codex")
    }

    pub(crate) fn launch_environment(&self) -> &[(String, String)] {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if let Some(fixture) = &self.desktop_fixture {
            return &fixture.environment;
        }
        self.observed
            .as_ref()
            .map(|observed| observed.environment.as_slice())
            .unwrap_or_else(|| self.environment().entries())
    }

    pub(crate) fn capability_ceiling(
        &self,
    ) -> Result<crate::acp::intent::CapabilitySnapshotV1, CodexAdmissionUnavailable> {
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if self.is_desktop_fixture() {
            return super::super::degrade::projected_desktop_fixture_capabilities(
                self.packet.capability_ceiling(),
            )
            .map_err(CodexAdmissionUnavailable::from);
        }
        super::super::degrade::projected_capabilities(
            self.packet.capability_ceiling(),
            self.observed_build(),
        )
        .map(|(snapshot, _)| snapshot)
        .map_err(CodexAdmissionUnavailable::from)
    }

    pub(crate) fn model_discovery_available(&self) -> Result<bool, CodexAdmissionUnavailable> {
        // A desktop replay offers a model choice only when its session runs
        // the model it is opened with; its catalogue is the case's own.
        #[cfg(all(windows, feature = "acp-fixture", debug_assertions))]
        if self.is_desktop_fixture() {
            return Ok(self
                .desktop_fixture_case()
                .is_some_and(crate::acp_protocol::native_cases::native_case_offers_models));
        }
        if self.is_desktop_fixture() {
            return Ok(false);
        }
        super::super::degrade::projected_capabilities(
            self.packet.capability_ceiling(),
            self.observed_build(),
        )
        .map(|(_, available)| available)
        .map_err(CodexAdmissionUnavailable::from)
    }
}
