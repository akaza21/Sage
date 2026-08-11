use std::{path::PathBuf, sync::Arc};

use sha2::{Digest, Sha256};

use super::{
    authority::{CodexAuthorityPrerequisite, CodexLaunchEnvironment},
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
    _cleanup: TestCleanup,
}

impl AdmittedCodexBinding {
    #[cfg(all(test, windows))]
    fn admit_for_test(
        packet: MetadataValidatedPacket,
        reproduced: ReproducedPackagePrerequisite,
        authority: CodexAuthorityPrerequisite,
        environment: CodexLaunchEnvironment,
        cleanup: TestCleanup,
    ) -> Result<Self, CodexAdmissionUnavailable> {
        let entrypoint = entrypoint_identity(&packet)?;
        let binding = Self {
            packet,
            reproduced,
            authority,
            environment,
            entrypoint,
            _cleanup: cleanup,
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

    pub(crate) fn entrypoint(&self) -> &EntrypointIdentity {
        &self.entrypoint
    }

    pub(crate) fn packet(&self) -> &MetadataValidatedPacket {
        &self.packet
    }

    pub(crate) fn environment(&self) -> &CodexLaunchEnvironment {
        &self.environment
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

#[derive(Debug, Clone)]
pub(crate) struct CodexBindingSnapshot {
    binding: Option<Arc<AdmittedCodexBinding>>,
    unavailable: Option<CodexAdmissionUnavailable>,
}

impl CodexBindingSnapshot {
    pub(crate) fn binding(&self) -> Option<&Arc<AdmittedCodexBinding>> {
        self.binding.as_ref()
    }

    pub(crate) fn unavailable(&self) -> Option<CodexAdmissionUnavailable> {
        self.unavailable
    }

    #[cfg(all(test, windows))]
    pub(crate) fn admitted_for_test() -> Self {
        Self {
            binding: Some(Arc::new(admitted_binding_fixture())),
            unavailable: None,
        }
    }
}

pub(crate) fn startup_codex_binding_snapshot() -> CodexBindingSnapshot {
    let unavailable = if cfg!(windows) {
        CodexAdmissionUnavailable::IndependentAdmission
    } else {
        CodexAdmissionUnavailable::Platform
    };
    CodexBindingSnapshot {
        binding: None,
        unavailable: Some(unavailable),
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

#[cfg(all(test, windows))]
#[derive(Debug)]
struct TestCleanup(PathBuf);

#[cfg(all(test, windows))]
impl Drop for TestCleanup {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(all(test, windows))]
fn admitted_binding_fixture() -> AdmittedCodexBinding {
    use serde_json::{Value, json};

    const PACKET: &[u8] = include_bytes!("../../../../testdata/acp/codex/candidate-windows.json");
    const POLICY: &[u8] = include_bytes!("../../../../testdata/acp/codex/security-policy-v1.json");
    const CONFIG: &[u8] = include_bytes!("../../../../testdata/acp/codex/config-v1.toml");

    let parent =
        std::env::temp_dir().join(format!("sage-codex-admission-{}", uuid::Uuid::new_v4()));
    let home = parent
        .join("CodexState")
        .join("codex-app-server-0.145.0-7b0ab167");
    std::fs::create_dir_all(home.join("sessions")).unwrap();
    std::fs::write(home.join("config.toml"), CONFIG).unwrap();

    let mut packet_value: Value = serde_json::from_slice(PACKET).unwrap();
    packet_value["codexHome"] = json!(home.to_str().unwrap());
    let packet = super::parse_packet_metadata(&serde_json::to_vec(&packet_value).unwrap()).unwrap();
    let mut policy_value: Value = serde_json::from_slice(POLICY).unwrap();
    policy_value["codexHome"] = json!(home.to_str().unwrap());
    let policy =
        super::parse_security_policy(&serde_json::to_vec(&policy_value).unwrap(), &packet).unwrap();
    let config = super::verify_codex_config(CONFIG, &policy).unwrap();
    let reproduced = super::reproduction::ReproducedPackagePrerequisite::fixture(&packet);
    let managed = super::managed::ManagedStagingPrerequisite::fixture(&packet, &reproduced);
    let environment =
        super::authority::build_system_codex_launch_environment(&packet, &policy, &config).unwrap();
    let authority =
        super::inspect_codex_authority(&packet, policy, config, &reproduced, managed).unwrap();
    AdmittedCodexBinding::admit_for_test(
        packet,
        reproduced,
        authority,
        environment,
        TestCleanup(parent),
    )
    .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_snapshot_is_typed_unavailable_without_independent_admission() {
        let snapshot = startup_codex_binding_snapshot();

        assert!(snapshot.binding().is_none());
        assert_eq!(
            snapshot.unavailable(),
            Some(if cfg!(windows) {
                CodexAdmissionUnavailable::IndependentAdmission
            } else {
                CodexAdmissionUnavailable::Platform
            })
        );
    }

    #[cfg(windows)]
    #[test]
    fn admitted_binding_owns_and_revalidates_every_launch_prerequisite() {
        let binding = admitted_binding_fixture();

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
}
