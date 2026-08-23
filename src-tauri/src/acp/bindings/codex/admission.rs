use std::{
    path::PathBuf,
    sync::{Arc, Mutex, MutexGuard},
};

use sha2::{Digest, Sha256};

use super::{
    authority::{CodexAuthorityPrerequisite, CodexLaunchEnvironment},
    installed_role::SignedInstalledRole,
    manifest::MetadataValidatedPacket,
    reproduction::ReproducedPackagePrerequisite,
};

const BINDING_ID: &str = "codex-app-server";
const BINDING_DIGEST_INPUT: &[u8] = b"sage:codex-app-server:0.145.0:windows-x86_64:7b0ab1679be705644a1b6c3f486f15e2f90e5f900486d90073b1dc5f0cf4c62c";
const BINDING_DIGEST: [u8; 32] = [
    0xf8, 0xc2, 0xb9, 0x95, 0xbe, 0x26, 0x3f, 0xe4, 0xbf, 0x00, 0x97, 0x40, 0x43, 0x31, 0x26, 0x73,
    0x37, 0x80, 0x81, 0x73, 0x4c, 0xa8, 0xc9, 0x81, 0x55, 0xd2, 0xa3, 0x38, 0x87, 0x44, 0x4b, 0x9e,
];
const BINDING_DIGEST_HEX: &str = "f8c2b995be263fe4bf00974043312673378081734ca8c98155d2a33887444b9e";

#[derive(Debug)]
pub(crate) struct EntrypointIdentity {
    pub(crate) path: PathBuf,
    pub(crate) sha256: [u8; 32],
    pub(crate) byte_len: u64,
}

#[derive(Debug)]
pub(crate) struct AdmittedCodexBinding {
    packet: MetadataValidatedPacket,
    reproduced: ReproducedPackagePrerequisite,
    authority: CodexAuthorityPrerequisite,
    environment: CodexLaunchEnvironment,
    entrypoint: EntrypointIdentity,
    #[cfg(all(test, windows))]
    _cleanup: Option<authenticate_tests::TestCleanup>,
}

impl AdmittedCodexBinding {
    fn admit_signed_installed_role(
        packet: MetadataValidatedPacket,
        role: SignedInstalledRole,
        reproduced: ReproducedPackagePrerequisite,
        authority: CodexAuthorityPrerequisite,
        environment: CodexLaunchEnvironment,
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
        let binding = Self {
            packet,
            reproduced,
            authority,
            environment,
            entrypoint,
            #[cfg(all(test, windows))]
            _cleanup: None,
        };
        binding.revalidate()?;
        Ok(binding)
    }

    pub(crate) fn revalidate(&self) -> Result<(), CodexAdmissionUnavailable> {
        self.reproduced
            .revalidate(&self.packet)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.authority
            .revalidate(&self.packet, &self.reproduced)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.environment
            .revalidate(&self.packet)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        let current = entrypoint_identity(&self.packet)?;
        if current.path != self.entrypoint.path
            || current.sha256 != self.entrypoint.sha256
            || current.byte_len != self.entrypoint.byte_len
            || self.packet.metadata().binding_id != BINDING_ID
            || <[u8; 32]>::from(Sha256::digest(BINDING_DIGEST_INPUT)) != BINDING_DIGEST
        {
            return Err(CodexAdmissionUnavailable::Drift);
        }
        Ok(())
    }

    pub(crate) fn binding_id(&self) -> &str {
        BINDING_ID
    }

    pub(crate) fn binding_digest(&self) -> [u8; 32] {
        BINDING_DIGEST
    }

    pub(crate) fn binding_digest_hex(&self) -> &str {
        BINDING_DIGEST_HEX
    }

    pub(crate) fn artifact_digest(&self) -> &str {
        &self.packet.metadata().entrypoint.sha256
    }

    pub(crate) fn harness_version(&self) -> &str {
        &self.packet.metadata().codex_runtime_version
    }

    pub(crate) fn entrypoint(&self) -> &EntrypointIdentity {
        &self.entrypoint
    }

    pub(crate) fn packet(&self) -> &MetadataValidatedPacket {
        &self.packet
    }

    pub(crate) fn environment(&self) -> &CodexLaunchEnvironment {
        &self.environment
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum CodexAdmissionUnavailable {
    #[error("Codex Harness Admission is unavailable on this platform")]
    Platform,
    #[error("the exact Codex Binding has not completed independent Harness Admission")]
    IndependentAdmission,
    #[error("the admitted Codex Binding changed after admission")]
    Drift,
}

#[derive(Debug)]
enum CodexRegistrationState {
    Admitted(Arc<AdmittedCodexBinding>),
    Unavailable(CodexAdmissionUnavailable),
}

#[derive(Debug)]
struct CodexRegistrationAuthority {
    state: Mutex<CodexRegistrationState>,
}

#[derive(Debug, Clone)]
pub(crate) struct CodexBindingRegistration {
    authority: Arc<CodexRegistrationAuthority>,
}

impl CodexBindingRegistration {
    pub(crate) fn authenticate_signed_installed_role(
        &self,
        packet: &MetadataValidatedPacket,
        role: SignedInstalledRole,
        reproduced: ReproducedPackagePrerequisite,
        authority: CodexAuthorityPrerequisite,
        environment: CodexLaunchEnvironment,
    ) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = Arc::new(
            AdmittedCodexBinding::admit_signed_installed_role(
                packet.clone(),
                role,
                reproduced,
                authority,
                environment,
            )
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?,
        );
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            ) => {
                *state = CodexRegistrationState::Admitted(Arc::clone(&candidate));
                Ok(candidate)
            }
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
            CodexRegistrationState::Admitted(binding) => Ok(Arc::clone(binding)),
        }
    }

    pub(crate) fn is_available(&self) -> bool {
        matches!(*self.lock_state(), CodexRegistrationState::Admitted(_))
    }

    pub(crate) fn unavailable(&self) -> Option<CodexAdmissionUnavailable> {
        match *self.lock_state() {
            CodexRegistrationState::Admitted(_) => None,
            CodexRegistrationState::Unavailable(reason) => Some(reason),
        }
    }

    pub(crate) fn refresh(&self) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = match &*self.lock_state() {
            CodexRegistrationState::Admitted(binding) => Arc::clone(binding),
            CodexRegistrationState::Unavailable(reason) => return Err(*reason),
        };
        let validation = candidate.revalidate();
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Admitted(current) if Arc::ptr_eq(current, &candidate) => {
                if validation.is_err() {
                    *state = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
                    Err(CodexAdmissionUnavailable::Drift)
                } else {
                    Ok(candidate)
                }
            }
            CodexRegistrationState::Admitted(_) => Err(CodexAdmissionUnavailable::Drift),
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
        }
    }

    pub(crate) fn invalidate_drift(&self) {
        *self.lock_state() = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
    }

    fn lock_state(&self) -> MutexGuard<'_, CodexRegistrationState> {
        match self.authority.state.lock() {
            Ok(state) => state,
            Err(poisoned) => {
                let mut state = poisoned.into_inner();
                *state = CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Drift);
                state
            }
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn unavailable_for_test() -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Unavailable(
                    CodexAdmissionUnavailable::IndependentAdmission,
                )),
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn admitted_for_test() -> Self {
        Self {
            authority: Arc::new(CodexRegistrationAuthority {
                state: Mutex::new(CodexRegistrationState::Admitted(Arc::new(
                    authenticate_tests::admitted_binding_fixture(),
                ))),
            }),
        }
    }

    #[cfg(all(test, windows))]
    pub(crate) fn induce_drift_for_test(&self) {
        let binding = match &*self.lock_state() {
            CodexRegistrationState::Admitted(binding) => Arc::clone(binding),
            CodexRegistrationState::Unavailable(_) => panic!("registration already unavailable"),
        };
        std::fs::write(
            binding
                .packet()
                .codex_home()
                .join("sessions")
                .join("registration-drift"),
            b"drift",
        )
        .unwrap();
    }

    #[cfg(test)]
    pub(crate) fn same_authority(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.authority, &other.authority)
    }
}

pub(crate) fn startup_codex_binding_registration() -> CodexBindingRegistration {
    let unavailable = if cfg!(windows) {
        CodexAdmissionUnavailable::IndependentAdmission
    } else {
        CodexAdmissionUnavailable::Platform
    };
    let registration = CodexBindingRegistration {
        authority: Arc::new(CodexRegistrationAuthority {
            state: Mutex::new(CodexRegistrationState::Unavailable(unavailable)),
        }),
    };
    #[cfg(windows)]
    {
        let _ = authenticate_startup_signed_installed_role(&registration);
    }
    registration
}

#[cfg(windows)]
fn authenticate_startup_signed_installed_role(
    registration: &CodexBindingRegistration,
) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
    let packet = super::parse_packet_metadata(include_bytes!(
        "../../../../testdata/acp/codex/candidate-windows.json"
    ))
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let role = packet
        .require_physical_admission()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let requirements = super::parse_reproduction_requirements(
        include_bytes!("../../../../testdata/acp/codex/package-reproduction-v1.json"),
        &packet,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let root = super::ConfiguredManagedPackageRoot::new(packet.managed_package_root())
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let inspection = super::inspect_managed_package(&packet, &root)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let reproduced = super::bind_reproduced_package(requirements, &packet, inspection)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let managed = super::inspect_managed_staging(&packet, &reproduced)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let policy = super::parse_security_policy(
        include_bytes!("../../../../testdata/acp/codex/security-policy-v1.json"),
        &packet,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let config = super::verify_codex_config(
        include_bytes!("../../../../testdata/acp/codex/config-v1.toml"),
        &policy,
    )
    .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let environment =
        super::authority::build_system_codex_launch_environment(&packet, &policy, &config)
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let authority = super::inspect_codex_authority(&packet, policy, config, &reproduced, managed)
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;

    registration.authenticate_signed_installed_role(
        &packet,
        role,
        reproduced,
        authority,
        environment,
    )
}

fn entrypoint_identity(
    packet: &MetadataValidatedPacket,
) -> Result<EntrypointIdentity, CodexAdmissionUnavailable> {
    let entrypoint = &packet.metadata().entrypoint;
    Ok(EntrypointIdentity {
        path: entrypoint.path.clone(),
        sha256: decode_sha256(&entrypoint.sha256)?,
        byte_len: entrypoint.byte_len,
    })
}

fn decode_sha256(value: &str) -> Result<[u8; 32], CodexAdmissionUnavailable> {
    if value.len() != 64 {
        return Err(CodexAdmissionUnavailable::Drift);
    }
    let mut digest = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        digest[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(digest)
}

fn hex_nibble(value: u8) -> Result<u8, CodexAdmissionUnavailable> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(CodexAdmissionUnavailable::Drift),
    }
}

#[cfg(all(test, windows))]
#[path = "admission/authenticate_tests.rs"]
mod authenticate_tests;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_registration_is_typed_unavailable_or_refreshable_after_admission() {
        let registration = startup_codex_binding_registration();
        let separately_started = startup_codex_binding_registration();

        assert!(!registration.same_authority(&separately_started));
        #[cfg(not(windows))]
        assert_eq!(
            registration.unavailable(),
            Some(CodexAdmissionUnavailable::Platform)
        );
        #[cfg(windows)]
        match registration.unavailable() {
            Some(unavailable) => {
                assert_eq!(unavailable, CodexAdmissionUnavailable::IndependentAdmission)
            }
            None => {
                assert!(registration.is_available());
                registration.refresh().unwrap();
            }
        }
    }

    #[cfg(windows)]
    #[test]
    fn admitted_binding_owns_and_revalidates_every_launch_prerequisite() {
        let binding = authenticate_tests::admitted_binding_fixture();

        binding.revalidate().unwrap();
        assert_eq!(binding.binding_id(), "codex-app-server");
        assert_eq!(
            binding.binding_digest_hex(),
            "f8c2b995be263fe4bf00974043312673378081734ca8c98155d2a33887444b9e"
        );
        assert_eq!(
            binding.artifact_digest(),
            "83751f15cb6a0a7b97df67752c001e3fe1c20e18ffbfec3ff63567296205eb6c"
        );
        assert_eq!(binding.entrypoint().byte_len, 359_245_096);
    }

    #[cfg(windows)]
    #[test]
    fn concurrent_refreshes_converge_on_one_fail_closed_drift_transition() {
        let registration = CodexBindingRegistration::admitted_for_test();
        registration.induce_drift_for_test();
        let gate = Arc::new(std::sync::Barrier::new(5));
        let workers = (0..4)
            .map(|_| {
                let registration = registration.clone();
                let gate = Arc::clone(&gate);
                std::thread::spawn(move || {
                    gate.wait();
                    registration.refresh().err()
                })
            })
            .collect::<Vec<_>>();
        gate.wait();

        for worker in workers {
            assert_eq!(
                worker.join().unwrap(),
                Some(CodexAdmissionUnavailable::Drift)
            );
        }
        assert!(!registration.is_available());
        assert_eq!(
            registration.unavailable(),
            Some(CodexAdmissionUnavailable::Drift)
        );
    }
}
