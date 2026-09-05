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

    pub(crate) fn entrypoint(&self) -> &EntrypointIdentity {
        &self.entrypoint
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
        self.observed
            .as_ref()
            .map(|observed| observed.environment.as_slice())
            .unwrap_or_else(|| self.environment().entries())
    }

    pub(crate) fn capability_ceiling(
        &self,
    ) -> Result<crate::acp::intent::CapabilitySnapshotV1, CodexAdmissionUnavailable> {
        use crate::acp::{
            contract::{RequiredText, SchemaVersionV1},
            intent::CapabilitySnapshotV1,
        };

        let value = self.packet.capability_ceiling();
        Ok(CapabilitySnapshotV1 {
            schema_version: SchemaVersionV1,
            protocol_version: RequiredText::new("protocolVersion", &value.protocol_identity)
                .map_err(|_| CodexAdmissionUnavailable::Drift)?,
            new_session: value.new_session,
            resume_session: value.resume_session,
            load_session: value.load_session,
            close_session: value.close_session,
            prompt: value.prompt,
            cancel: value.cancel,
            native_session_fork: false,
            active_turn_steer: false,
            child_conversation_projection: value.child_conversation_projection,
            child_transcript_read: false,
            collaboration_event_projection: value.collaboration_event_projection,
            multi_root_extension_schema_id: None,
            model_count: 0,
            config_count: 0,
        })
    }
}
