use std::path::{Path, PathBuf};

use serde::Deserialize;

use super::{
    installed_role::{SignedInstalledRole, admit_installed_role},
    trust_root::{WinVerifyTrustCatalog, verify_installed_authenticode},
};

const MANIFEST_BYTES: usize = 64 * 1024;
const ARTIFACTS_MAX: usize = 64;
const ARTIFACT_BYTES_MAX: u64 = 1024 * 1024 * 1024;
const PATH_BYTES_MAX: usize = 32 * 1024;
const PINNED_CODEX_COMMIT: &str = "e363b08c9175ac1cbe5893615dd2cb9ddf95043b";
const PINNED_CODEX_VERSION: &str = "0.146.0";
const PINNED_PACKAGE_NAME: &str = "@openai/codex";
const PINNED_PACKAGE_VERSION: &str = "0.146.0-win32-x64";
const PINNED_PACKAGE_SHA1: &str = "0a0f939280e71ea262c87751ccdb6a17d5bff81b";
const PINNED_PACKAGE_INTEGRITY: &str = "sha512-b3lxMYeR0+IhstNo4JjX1P9cPc1xwVcCVkPd1lD1wpWPJ0SBhpIkPczwbu3ZRkJcdyl342+rgyf4DUrbZLdrGA==";
const PINNED_PACKAGE_PACKED_BYTES: u64 = 151_682_891;
const PINNED_PACKAGE_UNPACKED_BYTES: u64 = 429_289_628;
pub(crate) const PINNED_PROTOCOL_IDENTITY: &str = "codex-app-server-0.146.0-d3992fec";
pub(crate) fn supports_native_0146_protocol(identity: &str) -> bool {
    matches!(
        identity,
        PINNED_PROTOCOL_IDENTITY | "codex-app-server-observed-0.146.0"
    )
}
pub(crate) const PINNED_SCHEMA_SHA256: &str =
    "d3992fec1398afdbec658da2c720c6993fbf3c1ce4900785694d2196679eddfc";
const PINNED_CODEX_HOME_SUFFIX: &str = r"CodexState\codex-app-server-0.146.0-d3992fec";

#[derive(Clone, Copy)]
struct ExpectedArtifact {
    relative_path: &'static str,
    sha256: &'static str,
    byte_len: u64,
}

const PINNED_ARTIFACTS: [ExpectedArtifact; 8] = [
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\bin\codex.exe",
        sha256: "bc343ba420dc2e2e9f59e6fc5e5bf0aae1cd8c771fc319665241fc9c0271fddb",
        byte_len: 358_650_672,
    },
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\bin\codex-code-mode-host.exe",
        sha256: "6ef1de0e04d859f8f4f6d4d64f0f3ceeec28658423d91de160f5e804280d1c36",
        byte_len: 56_309_040,
    },
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-command-runner.exe",
        sha256: "0102fa1820ecd03bb03a991fd2303a1a484118f7da8a71864f88ec94bca61d6d",
        byte_len: 1_302_832,
    },
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\codex-resources\codex-windows-sandbox-setup.exe",
        sha256: "c12d225b34e7f82cdab6bbc714797abed661f40e158104694953889750121cef",
        byte_len: 8_804_144,
    },
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\codex-path\rg.exe",
        sha256: "14231169855ec5205cf5a1b6f1db358ff4aed4247c86b69ce8aae647c77f6680",
        byte_len: 4_218_880,
    },
    ExpectedArtifact {
        relative_path: r"vendor\x86_64-pc-windows-msvc\codex-package.json",
        sha256: "aaa0646d6b615da94187b51efd50c69621a00867761161ae55cc16cfd545bec7",
        byte_len: 215,
    },
    ExpectedArtifact {
        relative_path: "package.json",
        sha256: "223e8306038c65e658e6d008ae50b45d238e0a6778acd88b70c4f9d1534f3a2f",
        byte_len: 511,
    },
    ExpectedArtifact {
        relative_path: "README.md",
        sha256: "ba4e1f69ff48386e72a9c5e1edaf76aad64a475c2d51af79ccba6d1128261ba7",
        byte_len: 3_334,
    },
];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Platform {
    WindowsX86_64,
    LinuxX86_64,
    MacosAarch64,
    MacosX86_64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct Artifact {
    pub path: PathBuf,
    pub sha256: String,
    pub byte_len: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CuratedPacketMetadata {
    pub schema_version: u8,
    pub binding_id: String,
    pub platform: Platform,
    pub codex_source_commit: String,
    pub codex_runtime_version: String,
    pub package_name: String,
    pub package_version: String,
    pub package_tarball_sha1: String,
    pub package_integrity_sha512: String,
    pub package_packed_byte_len: u64,
    pub package_unpacked_byte_len: u64,
    pub app_server_schema_sha256: String,
    pub entrypoint: Artifact,
    pub supporting_artifacts: Vec<Artifact>,
    pub codex_home: PathBuf,
    pub state_policy: StatePolicy,
    pub transport: Transport,
    pub workspace_policy: WorkspacePolicy,
    pub capability_ceiling: CuratedCapabilityCeiling,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct CuratedCapabilityCeiling {
    pub protocol_identity: String,
    pub new_session: bool,
    pub resume_session: bool,
    pub load_session: bool,
    pub close_session: bool,
    pub prompt: bool,
    pub cancel: bool,
    pub model_list: bool,
    pub child_conversation_projection: bool,
    pub collaboration_event_projection: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum StatePolicy {
    IsolatedBindingDirectory,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum Transport {
    StdioJsonl,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) enum WorkspacePolicy {
    SingleRootOnly,
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// A bounded packet whose declared metadata matches Sage's exact lexical policy.
///
/// This type has not inspected the physical files. In particular, it does not
/// prove canonical paths, file hashes, signatures, reparse-point safety, ACLs,
/// immutability, or launch-time identity.
pub(crate) struct MetadataValidatedPacket(CuratedPacketMetadata);

impl MetadataValidatedPacket {
    pub(crate) fn metadata(&self) -> &CuratedPacketMetadata {
        &self.0
    }

    pub(crate) fn require_physical_admission(
        &self,
    ) -> Result<SignedInstalledRole, PhysicalAdmissionUnavailable> {
        let entrypoint = &self.0.entrypoint;
        let signature = verify_installed_authenticode(
            &WinVerifyTrustCatalog,
            self,
            PINNED_ARTIFACTS[0].relative_path,
            &entrypoint.path,
        )
        .map_err(|_| PhysicalAdmissionUnavailable::ArtifactNotAdmitted)?;
        admit_installed_role(self, signature)
            .map_err(|_| PhysicalAdmissionUnavailable::ArtifactNotAdmitted)
    }

    pub(crate) fn codex_home(&self) -> &Path {
        &self.0.codex_home
    }

    pub(super) fn managed_package_root(&self) -> PathBuf {
        let mut root = self.0.entrypoint.path.clone();
        for _ in Path::new(PINNED_ARTIFACTS[0].relative_path).components() {
            let removed = root.pop();
            debug_assert!(removed, "validated entrypoint has the pinned suffix");
        }
        root
    }

    pub(crate) fn capability_ceiling(&self) -> &CuratedCapabilityCeiling {
        &self.0.capability_ceiling
    }

    pub(super) fn physical_artifact_claims(
        &self,
    ) -> impl Iterator<Item = (&Artifact, &'static str)> {
        std::iter::once((&self.0.entrypoint, PINNED_ARTIFACTS[0].relative_path)).chain(
            self.0
                .supporting_artifacts
                .iter()
                .zip(PINNED_ARTIFACTS.iter().skip(1))
                .map(|(artifact, expected)| (artifact, expected.relative_path)),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PhysicalAdmissionUnavailable {
    #[error("the exact Codex artifact has not been independently admitted")]
    ArtifactNotAdmitted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PacketMetadataError {
    #[error("packet metadata exceeds its byte limit")]
    ManifestBytes,
    #[error("packet metadata is malformed or has an unknown field")]
    Malformed,
    #[error("packet metadata identity is invalid")]
    Identity,
    #[error("packet artifact metadata is invalid")]
    Artifact,
}

/// Parses and validates packet claims without touching the filesystem.
pub(crate) fn parse_packet_metadata(
    bytes: &[u8],
) -> Result<MetadataValidatedPacket, PacketMetadataError> {
    if bytes.len() > MANIFEST_BYTES {
        return Err(PacketMetadataError::ManifestBytes);
    }
    let metadata: CuratedPacketMetadata =
        serde_json::from_slice(bytes).map_err(|_| PacketMetadataError::Malformed)?;
    validate_packet_metadata(&metadata)?;
    Ok(MetadataValidatedPacket(metadata))
}

fn validate_packet_metadata(metadata: &CuratedPacketMetadata) -> Result<(), PacketMetadataError> {
    if metadata.schema_version != 1
        || metadata.binding_id != "codex-app-server"
        || metadata.platform != Platform::WindowsX86_64
        || metadata.codex_source_commit != PINNED_CODEX_COMMIT
        || metadata.codex_runtime_version != PINNED_CODEX_VERSION
        || metadata.package_name != PINNED_PACKAGE_NAME
        || metadata.package_version != PINNED_PACKAGE_VERSION
        || metadata.package_tarball_sha1 != PINNED_PACKAGE_SHA1
        || metadata.package_integrity_sha512 != PINNED_PACKAGE_INTEGRITY
        || metadata.package_packed_byte_len != PINNED_PACKAGE_PACKED_BYTES
        || metadata.package_unpacked_byte_len != PINNED_PACKAGE_UNPACKED_BYTES
        || metadata.app_server_schema_sha256 != PINNED_SCHEMA_SHA256
        || metadata.supporting_artifacts.len() != PINNED_ARTIFACTS.len() - 1
        || metadata.supporting_artifacts.len() > ARTIFACTS_MAX - 1
        || metadata.capability_ceiling != pinned_capability_ceiling()
    {
        return Err(PacketMetadataError::Identity);
    }

    let mut total = validate_artifact(&metadata.entrypoint, metadata.platform)?;
    let package_root = validate_expected_artifact(&metadata.entrypoint, PINNED_ARTIFACTS[0])?;
    let mut paths = vec![normalized_path_key(
        &metadata.entrypoint.path,
        metadata.platform,
    )];
    for (artifact, expected) in metadata
        .supporting_artifacts
        .iter()
        .zip(PINNED_ARTIFACTS.iter().skip(1))
    {
        total = total
            .checked_add(validate_artifact(artifact, metadata.platform)?)
            .ok_or(PacketMetadataError::Artifact)?;
        if validate_expected_artifact(artifact, *expected)? != package_root {
            return Err(PacketMetadataError::Artifact);
        }
        let key = normalized_path_key(&artifact.path, metadata.platform);
        if paths.iter().any(|existing| existing == &key) {
            return Err(PacketMetadataError::Artifact);
        }
        paths.push(key);
    }
    if total > ARTIFACT_BYTES_MAX || total != metadata.package_unpacked_byte_len {
        return Err(PacketMetadataError::Artifact);
    }
    validate_codex_home(&metadata.codex_home, &package_root)?;
    Ok(())
}

fn pinned_capability_ceiling() -> CuratedCapabilityCeiling {
    CuratedCapabilityCeiling {
        protocol_identity: PINNED_PROTOCOL_IDENTITY.into(),
        new_session: true,
        resume_session: true,
        load_session: true,
        close_session: true,
        prompt: true,
        cancel: true,
        model_list: true,
        child_conversation_projection: true,
        collaboration_event_projection: true,
    }
}

fn validate_expected_artifact(
    artifact: &Artifact,
    expected: ExpectedArtifact,
) -> Result<String, PacketMetadataError> {
    if artifact.sha256 != expected.sha256 || artifact.byte_len != expected.byte_len {
        return Err(PacketMetadataError::Artifact);
    }
    let value = normalized_path_key(&artifact.path, Platform::WindowsX86_64);
    let suffix = format!(r"\{}", expected.relative_path.to_ascii_lowercase());
    let root_len = value
        .len()
        .checked_sub(suffix.len())
        .filter(|_| value.ends_with(&suffix))
        .ok_or(PacketMetadataError::Artifact)?;
    let root = value[..root_len].to_owned();
    if root.is_empty() || root.ends_with('\\') {
        return Err(PacketMetadataError::Artifact);
    }
    Ok(root)
}

fn validate_codex_home(path: &Path, package_root: &str) -> Result<(), PacketMetadataError> {
    let raw = path.as_os_str().to_string_lossy();
    if raw.is_empty()
        || raw.len() > PATH_BYTES_MAX
        || !absolute_normalized(path, Platform::WindowsX86_64)
    {
        return Err(PacketMetadataError::Artifact);
    }
    let home = normalized_path_key(path, Platform::WindowsX86_64);
    if !home.ends_with(&format!(
        r"\{}",
        PINNED_CODEX_HOME_SUFFIX.to_ascii_lowercase()
    )) || same_or_descendant(&home, package_root)
        || same_or_descendant(package_root, &home)
    {
        return Err(PacketMetadataError::Artifact);
    }
    Ok(())
}

fn same_or_descendant(path: &str, parent: &str) -> bool {
    path == parent || path.starts_with(&format!(r"{parent}\"))
}

fn validate_artifact(artifact: &Artifact, platform: Platform) -> Result<u64, PacketMetadataError> {
    let path = artifact.path.as_os_str().to_string_lossy();
    if path.is_empty()
        || path.len() > PATH_BYTES_MAX
        || artifact.byte_len == 0
        || artifact.byte_len > ARTIFACT_BYTES_MAX
        || !valid_hex(&artifact.sha256, 64)
        || !absolute_normalized(&artifact.path, platform)
    {
        return Err(PacketMetadataError::Artifact);
    }
    Ok(artifact.byte_len)
}

fn absolute_normalized(path: &Path, platform: Platform) -> bool {
    let raw = path.as_os_str().to_string_lossy();
    match platform {
        Platform::WindowsX86_64 => {
            let bytes = raw.as_bytes();
            bytes.len() >= 4
                && bytes[0].is_ascii_alphabetic()
                && bytes[1] == b':'
                && bytes[2] == b'\\'
                && !raw.contains('/')
                && !raw.split('\\').enumerate().any(|(index, part)| {
                    part.is_empty()
                        || matches!(part, "." | "..")
                        || (index == 0 && part.len() != 2)
                        || (index > 0
                            && (part.contains(':') || part.ends_with('.') || part.ends_with(' ')))
                })
        }
        Platform::LinuxX86_64 | Platform::MacosAarch64 | Platform::MacosX86_64 => {
            raw.starts_with('/')
                && !raw
                    .split('/')
                    .skip(1)
                    .any(|part| part.is_empty() || matches!(part, "." | ".."))
        }
    }
}

fn normalized_path_key(path: &Path, platform: Platform) -> String {
    let value = path.as_os_str().to_string_lossy().into_owned();
    match platform {
        Platform::WindowsX86_64 => value.to_ascii_lowercase(),
        Platform::LinuxX86_64 | Platform::MacosAarch64 | Platform::MacosX86_64 => value,
    }
}

fn valid_hex(value: &str, bytes: usize) -> bool {
    value.len() == bytes
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}
