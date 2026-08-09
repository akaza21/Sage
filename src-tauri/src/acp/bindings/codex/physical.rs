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
    artifact_count: usize,
    byte_len: u64,
    _held_handles: Vec<File>,
}

impl PhysicallyInspectedPackage {
    #[cfg(test)]
    fn package_root(&self) -> &Path {
        &self.package_root
    }

    #[cfg(test)]
    fn artifact_count(&self) -> usize {
        self.artifact_count
    }

    #[cfg(test)]
    fn byte_len(&self) -> u64 {
        self.byte_len
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
struct ExpectedPhysicalFile {
    pub(super) relative_path: PathBuf,
    pub(super) sha256: [u8; 32],
    pub(super) byte_len: u64,
}

#[derive(Debug)]
struct InspectionClaims {
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

#[cfg(windows)]
fn inspect_claims(
    claims: InspectionClaims,
) -> Result<PhysicallyInspectedPackage, PhysicalInspectionError> {
    windows::inspect_claims(claims)
}

fn decode_digest(value: &str) -> Result<[u8; 32], PhysicalInspectionError> {
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
