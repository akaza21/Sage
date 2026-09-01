use std::fs::File;
use std::path::{Path, PathBuf};

use super::manifest::{MetadataValidatedPacket, Platform};

#[cfg(windows)]
#[path = "physical/windows.rs"]
mod windows;

const PACKAGE_ENTRY_MAX: usize = 64;
const PACKAGE_DEPTH_MAX: usize = 8;
const EXPECTED_FILE_COUNT: usize = 8;
const CONFIGURED_PATH_BYTES_MAX: usize = 32 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredManagedPackageRoot(PathBuf);

impl ConfiguredManagedPackageRoot {
    pub(crate) fn new(path: PathBuf) -> Result<Self, PhysicalInspectionError> {
        if path.as_os_str().is_empty() || path.as_os_str().len() > CONFIGURED_PATH_BYTES_MAX {
            return Err(PhysicalInspectionError::ConfiguredRoot);
        }
        Ok(Self(path))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ConfiguredCodexSourceRoot(PathBuf);

impl ConfiguredCodexSourceRoot {
    pub(crate) fn new(path: PathBuf) -> Result<Self, PhysicalInspectionError> {
        if path.as_os_str().is_empty() || path.as_os_str().len() > CONFIGURED_PATH_BYTES_MAX {
            return Err(PhysicalInspectionError::ConfiguredRoot);
        }
        Ok(Self(path))
    }

    fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Proof that one exact package closure was physically inspected.
///
/// Known nodes remain open and the directory namespace matched across two
/// bounded observations, but the namespace is not frozen and can change after
/// the final observation. This proof therefore grants no admission or launch
/// authority.
///
/// It also does not establish Authenticode or other signatures,
/// ACLs/ownership/immutability, provisioning, auth/state, process containment,
/// atomic check-to-launch, or macOS admission.
#[derive(Debug)]
pub(crate) struct PhysicallyInspectedPackage {
    package_root: PathBuf,
    codex_home: PathBuf,
    artifact_count: usize,
    byte_len: u64,
    held_nodes: Vec<HeldPhysicalNode>,
}

#[derive(Debug)]
pub(super) struct HeldPhysicalNode {
    pub(super) path: PathBuf,
    pub(super) directory: bool,
    pub(super) handle: File,
}

impl PhysicallyInspectedPackage {
    pub(super) fn binds_packet(&self, packet: &MetadataValidatedPacket) -> bool {
        #[cfg(windows)]
        {
            windows::proof_matches_packet(self, packet)
        }
        #[cfg(not(windows))]
        {
            #[cfg(test)]
            {
                const ENTRYPOINT_SUFFIX: &str = r"\vendor\x86_64-pc-windows-msvc\bin\codex.exe";
                let entrypoint = packet.metadata().entrypoint.path.to_string_lossy();
                let Some(package_root) = entrypoint.strip_suffix(ENTRYPOINT_SUFFIX) else {
                    return false;
                };
                return self.package_root == Path::new(package_root)
                    && self.codex_home == packet.codex_home()
                    && self.artifact_count == packet.metadata().supporting_artifacts.len() + 1
                    && self.byte_len == packet.metadata().package_unpacked_byte_len;
            }
            #[cfg(not(test))]
            {
                let _ = packet;
                false
            }
        }
    }

    #[cfg(test)]
    pub(super) fn mismatched_fixture() -> Self {
        Self {
            package_root: PathBuf::from(r"C:\Foreign\Codex\package"),
            codex_home: PathBuf::from(
                r"C:\SageHarnesses\CodexState\codex-app-server-0.145.0-7b0ab167",
            ),
            artifact_count: EXPECTED_FILE_COUNT,
            byte_len: 427_151_500,
            held_nodes: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(super) fn matching_fixture(packet: &MetadataValidatedPacket) -> Self {
        const ENTRYPOINT_SUFFIX: &str = r"\vendor\x86_64-pc-windows-msvc\bin\codex.exe";
        let entrypoint = packet.metadata().entrypoint.path.to_string_lossy();
        let package_root = entrypoint
            .strip_suffix(ENTRYPOINT_SUFFIX)
            .expect("pinned packet entrypoint suffix");
        Self {
            package_root: PathBuf::from(package_root),
            codex_home: packet.codex_home().to_owned(),
            artifact_count: packet.metadata().supporting_artifacts.len() + 1,
            byte_len: packet.metadata().package_unpacked_byte_len,
            held_nodes: Vec::new(),
        }
    }

    pub(super) fn package_root(&self) -> &Path {
        &self.package_root
    }

    pub(super) fn codex_home(&self) -> &Path {
        &self.codex_home
    }

    pub(super) fn artifact_count(&self) -> usize {
        self.artifact_count
    }

    pub(super) fn byte_len(&self) -> u64 {
        self.byte_len
    }

    pub(super) fn held_nodes(&self) -> &[HeldPhysicalNode] {
        &self.held_nodes
    }

    pub(super) fn root_node(&self) -> Option<&HeldPhysicalNode> {
        self.held_nodes
            .iter()
            .find(|node| node.directory && node.path == self.package_root)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum PhysicalInspectionError {
    #[error("physical Codex package inspection is unavailable on this platform")]
    PlatformUnavailable,
    #[error("the configured managed package root is invalid")]
    ConfiguredRoot,
    #[error("packet artifact paths do not match the configured package root")]
    PacketRootMismatch,
    #[error("the declared Codex state path is not canonical and disjoint")]
    StatePath,
    #[error("the package traversal exceeds its entry limit")]
    EntryLimit,
    #[error("the package traversal exceeds its depth limit")]
    DepthLimit,
    #[error("the physical package closure is incomplete or contains an unexpected entry")]
    Closure,
    #[error("the physical package contains a path alias")]
    Alias,
    #[error("a physical package entry has the wrong identity or type")]
    Identity,
    #[error("a physical package file digest does not match")]
    Digest,
    #[error("a physical package entry changed during inspection")]
    ChangedDuringInspection,
    #[error("physical package inspection failed")]
    Io,
}

#[derive(Debug, Clone)]
pub(super) struct ExpectedPhysicalFile {
    pub(super) relative_path: PathBuf,
    pub(super) sha256: [u8; 32],
    pub(super) byte_len: u64,
}

#[derive(Debug)]
pub(super) struct InspectionClaims {
    pub(super) package_root: PathBuf,
    pub(super) codex_home: PathBuf,
    pub(super) files: Vec<ExpectedPhysicalFile>,
    pub(super) aggregate_bytes: u64,
}

pub(crate) fn inspect_managed_package(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredManagedPackageRoot,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    if packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(PhysicalInspectionError::PlatformUnavailable);
    }
    #[cfg(windows)]
    {
        windows::inspect(packet, configured_root)
    }
    #[cfg(not(windows))]
    {
        let _ = configured_root;
        Err(PhysicalInspectionError::PlatformUnavailable)
    }
}

pub(crate) fn inspect_codex_source(
    packet: &MetadataValidatedPacket,
    configured_root: &ConfiguredCodexSourceRoot,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    if packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(PhysicalInspectionError::PlatformUnavailable);
    }
    #[cfg(windows)]
    {
        windows::inspect_source(packet, configured_root)
    }
    #[cfg(not(windows))]
    {
        let _ = configured_root;
        Err(PhysicalInspectionError::PlatformUnavailable)
    }
}

#[cfg(windows)]
pub(super) fn inspect_claims(
    claims: InspectionClaims,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    windows::inspect_claims(claims)
}

#[cfg(not(windows))]
pub(super) fn inspect_claims(
    _claims: InspectionClaims,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    Err(PhysicalInspectionError::PlatformUnavailable)
}

#[cfg(windows)]
pub(super) fn open_exact_state_root(
    path: &Path,
) -> Result<(PathBuf, File), PhysicalInspectionError> {
    windows::open_exact_state_root(path)
}

#[cfg(windows)]
pub(super) fn same_native_path(left: &Path, right: &Path) -> bool {
    windows::same_native_path(left, right)
}

pub(super) fn decode_digest(value: &str) -> Result<[u8; 32], PhysicalInspectionError> {
    if value.len() != 64 {
        return Err(PhysicalInspectionError::Identity);
    }
    let mut result = [0_u8; 32];
    for (index, pair) in value.as_bytes().chunks_exact(2).enumerate() {
        result[index] = (hex_nibble(pair[0])? << 4) | hex_nibble(pair[1])?;
    }
    Ok(result)
}

fn hex_nibble(value: u8) -> Result<u8, PhysicalInspectionError> {
    match value {
        b'0'..=b'9' => Ok(value - b'0'),
        b'a'..=b'f' => Ok(value - b'a' + 10),
        _ => Err(PhysicalInspectionError::Identity),
    }
}

#[cfg(all(test, windows))]
#[path = "physical/tests.rs"]
mod tests;

#[cfg(all(test, not(windows)))]
mod portable_tests {
    use super::*;

    #[test]
    fn physical_package_inspection_is_unavailable_off_windows() {
        let packet = crate::acp::bindings::codex::parse_packet_metadata(include_bytes!(
            "../../../../testdata/acp/codex/candidate-windows.json"
        ))
        .unwrap();
        let root = ConfiguredManagedPackageRoot::new(PathBuf::from(
            r"C:\SageHarnesses\Codex\0.145.0\package",
        ))
        .unwrap();
        assert_eq!(
            inspect_managed_package(&packet, &root).unwrap_err(),
            PhysicalInspectionError::PlatformUnavailable
        );
    }
}
