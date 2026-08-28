#[cfg(all(test, windows))]
use std::collections::VecDeque;
use std::{
    fs::File,
    io::Read,
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
    reproduced: Option<ReproducedPackagePrerequisite>,
    authority: Option<CodexAuthorityPrerequisite>,
    environment: Option<CodexLaunchEnvironment>,
    entrypoint: EntrypointIdentity,
    observed: Option<ObservedEntrypoint>,
    binding_digest: [u8; 32],
    binding_digest_hex: String,
    artifact_digest: String,
    harness_version: String,
    #[cfg(all(test, windows))]
    journey_test_entrypoint: Option<EntrypointIdentity>,
    #[cfg(all(test, windows))]
    journey_test_cases: Option<Mutex<VecDeque<&'static str>>>,
    #[cfg(all(test, windows))]
    _cleanup: Option<authenticate_tests::TestCleanup>,
}

#[derive(Debug)]
struct ObservedEntrypoint {
    environment: Vec<(String, String)>,
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
        executable: PathBuf,
        version: String,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        if version.is_empty() || version.len() > 512 {
            return Err(CodexAdmissionUnavailable::IndependentAdmission);
        }
        let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
            "../../../../testdata/acp/codex/candidate-windows.json"
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

    pub(crate) fn revalidate(&self) -> Result<(), CodexAdmissionUnavailable> {
        if let Some(observed) = &self.observed {
            let current = observed_entrypoint_identity(&self.entrypoint.path)?;
            return (current.sha256 == self.entrypoint.sha256
                && current.byte_len == self.entrypoint.byte_len
                && observed.environment == observed_environment(&self.entrypoint.path)?)
            .then_some(())
            .ok_or(CodexAdmissionUnavailable::Drift);
        }
        self.reproduced
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
            .revalidate(&self.packet)
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.authority
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
            .revalidate(
                &self.packet,
                self.reproduced
                    .as_ref()
                    .ok_or(CodexAdmissionUnavailable::Drift)?,
            )
            .map_err(|_| CodexAdmissionUnavailable::Drift)?;
        self.environment
            .as_ref()
            .ok_or(CodexAdmissionUnavailable::Drift)?
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

    /// The Windows journey uses the already-built ACP fixture only after this
    /// binding has passed normal admission revalidation. This deliberately
    /// does not alter the admitted packet, its digest, or the signed role.
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

    pub(crate) fn packet(&self) -> &MetadataValidatedPacket {
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

fn observed_entrypoint_identity(
    path: &std::path::Path,
) -> Result<EntrypointIdentity, CodexAdmissionUnavailable> {
    const MAX_OBSERVED_EXECUTABLE_BYTES: u64 = 512 * 1024 * 1024;
    let metadata = path
        .metadata()
        .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_OBSERVED_EXECUTABLE_BYTES
    {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    let mut file = File::open(path).map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
    let mut digest = Sha256::new();
    let mut chunk = [0_u8; 64 * 1024];
    loop {
        let read = file
            .read(&mut chunk)
            .map_err(|_| CodexAdmissionUnavailable::IndependentAdmission)?;
        if read == 0 {
            break;
        }
        digest.update(&chunk[..read]);
    }
    Ok(EntrypointIdentity {
        path: path.to_path_buf(),
        sha256: digest.finalize().into(),
        byte_len: metadata.len(),
    })
}

fn observed_binding_digest(entrypoint: &EntrypointIdentity) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(b"sage:observed-codex-app-server:v1:");
    digest.update(entrypoint.path.as_os_str().as_encoded_bytes());
    digest.update(entrypoint.sha256);
    digest.update(entrypoint.byte_len.to_le_bytes());
    digest.finalize().into()
}

fn sha256_hex(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn observed_environment(
    entrypoint: &std::path::Path,
) -> Result<Vec<(String, String)>, CodexAdmissionUnavailable> {
    const KEYS: &[&str] = &[
        "APPDATA",
        "ComSpec",
        "PATHEXT",
        "SystemRoot",
        "TEMP",
        "TMP",
        "USERPROFILE",
        "WINDIR",
    ];
    let mut entries = KEYS
        .iter()
        .map(|key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.is_empty())
                .map(|value| ((*key).to_owned(), value))
                .ok_or(CodexAdmissionUnavailable::IndependentAdmission)
        })
        .collect::<Result<Vec<_>, _>>()?;
    let node_directory = entrypoint
        .parent()
        .into_iter()
        .map(std::path::Path::to_path_buf)
        .chain(std::env::var("PATH").ok().into_iter().flat_map(|path| {
            path.split(';')
                .filter(|value| !value.is_empty())
                .map(std::path::PathBuf::from)
                .collect::<Vec<_>>()
        }))
        .find(|directory| directory.join("node.exe").is_file())
        .and_then(|directory| directory.to_str().map(str::to_owned))
        .ok_or(CodexAdmissionUnavailable::IndependentAdmission)?;
    entries.insert(0, ("PATH".into(), node_directory));
    if entries.len() > 16
        || entries
            .iter()
            .any(|(key, value)| key.len() > 128 || value.len() > 8_192)
    {
        return Err(CodexAdmissionUnavailable::IndependentAdmission);
    }
    Ok(entries)
}

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
    pub(crate) fn authenticate_observed(
        &self,
        executable: PathBuf,
        version: String,
    ) -> Result<Arc<AdmittedCodexBinding>, CodexAdmissionUnavailable> {
        let candidate = Arc::new(AdmittedCodexBinding::admit_observed(executable, version)?);
        let mut state = self.lock_state();
        match &*state {
            CodexRegistrationState::Unavailable(
                CodexAdmissionUnavailable::IndependentAdmission,
            )
            | CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::Platform) => {
                *state = CodexRegistrationState::Admitted(Arc::clone(&candidate));
                Ok(candidate)
            }
            CodexRegistrationState::Unavailable(reason) => Err(*reason),
            CodexRegistrationState::Admitted(binding) => Ok(Arc::clone(binding)),
        }
    }

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

    /// Authenticode can identify the installed role, but this workstation's
    /// curated evidence records it as observation-only. If independent launch
    /// admission cannot proceed, retain that more precise fail-closed state.
    fn refuse_independent_launch_after_workstation_authenticode_observation(
        &self,
    ) -> CodexAdmissionUnavailable {
        let mut state = self.lock_state();
        let unavailable = match &*state {
            CodexRegistrationState::Unavailable(reason) => *reason,
            CodexRegistrationState::Admitted(_) => return CodexAdmissionUnavailable::Drift,
        };
        if unavailable == CodexAdmissionUnavailable::IndependentAdmission {
            *state =
                CodexRegistrationState::Unavailable(CodexAdmissionUnavailable::ObservationOnly);
            CodexAdmissionUnavailable::ObservationOnly
        } else {
            unavailable
        }
    }
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

#[path = "admission/startup.rs"]
mod startup;
pub(crate) use startup::startup_codex_binding_registration;

#[cfg(test)]
#[path = "admission/test_registration.rs"]
mod test_registration;

#[cfg(all(test, windows))]
#[path = "admission/authenticate_tests.rs"]
mod authenticate_tests;

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    #[test]
    fn observed_binding_revalidates_the_exact_discovered_file_identity() {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        let path = std::env::temp_dir().join(format!("sage-observed-codex-{nonce}.bin"));
        fs::write(&path, b"observed-codex").expect("test executable");
        let binding = AdmittedCodexBinding::admit_observed(path.clone(), "codex 1.2.3".into())
            .expect("observed executable admission");
        assert_eq!(binding.entrypoint().path, path);
        binding.revalidate().expect("unchanged executable");
        fs::write(&path, b"changed-codex").expect("change test executable");
        assert_eq!(binding.revalidate(), Err(CodexAdmissionUnavailable::Drift));
        fs::remove_file(path).expect("remove test executable");
    }

    #[test]
    fn startup_registration_is_typed_unavailable_or_refreshable_after_independent_admission() {
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
            Some(CodexAdmissionUnavailable::ObservationOnly) => {}
            Some(CodexAdmissionUnavailable::IndependentAdmission) => {}
            Some(unavailable) => panic!("unexpected startup availability: {unavailable:?}"),
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
