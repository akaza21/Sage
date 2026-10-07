use std::path::Path;
use std::{fs::File, io::Read};

use sha2::{Digest, Sha256};

use super::manifest::MetadataValidatedPacket;
use super::reproduction::{
    authenticode_certificate_thumbprint_sha1, authenticode_publisher_common_name,
    authenticode_signed_relative_paths,
};

/// Installed Windows Authenticode publisher policy. Callers cannot supply
/// an ephemeral public key or thumbprint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InstalledAuthenticodePolicy(());

pub const INSTALLED_AUTHENTICODE_POLICY: InstalledAuthenticodePolicy =
    InstalledAuthenticodePolicy(());

impl InstalledAuthenticodePolicy {
    pub fn publisher_common_name(self) -> &'static str {
        authenticode_publisher_common_name()
    }

    pub fn certificate_thumbprint_sha1(self) -> &'static str {
        authenticode_certificate_thumbprint_sha1()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PacketArtifactBinding {
    binding_id: String,
    artifact_relative_path: String,
    artifact_sha256: String,
    artifact_byte_len: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub struct VerifiedSignature {
    binding: PacketArtifactBinding,
}

impl PacketArtifactBinding {
    pub fn artifact_sha256(&self) -> &str {
        &self.artifact_sha256
    }
}

impl VerifiedSignature {
    pub(crate) fn binding_id(&self) -> &str {
        &self.binding.binding_id
    }

    pub(crate) fn artifact_relative_path(&self) -> &str {
        &self.binding.artifact_relative_path
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum VerifyError {
    #[error("signature is not bound to packet and artifact identity")]
    Unbound,
    #[error("artifact is not covered by the installed Authenticode policy")]
    Artifact,
    #[error("WinVerifyTrust rejected the file")]
    Authenticode,
    #[error("installed trust root rejected the caller")]
    TrustRoot,
}

pub fn bind_packet_artifact(
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
) -> Result<PacketArtifactBinding, VerifyError> {
    if artifact_relative_path.is_empty() {
        return Err(VerifyError::Unbound);
    }
    let claim = packet
        .physical_artifact_claims()
        .find(|(_, relative)| *relative == artifact_relative_path)
        .ok_or(VerifyError::Artifact)?;
    if !authenticode_signed_relative_paths().contains(&artifact_relative_path) {
        return Err(VerifyError::Artifact);
    }
    Ok(PacketArtifactBinding {
        binding_id: packet.metadata().binding_id.clone(),
        artifact_relative_path: artifact_relative_path.to_owned(),
        artifact_sha256: claim.0.sha256.clone(),
        artifact_byte_len: claim.0.byte_len,
    })
}

/// Catalog Sage calls for Authenticode. Production uses WinVerifyTrust,
/// not detached RSA-over-bytes.
pub trait AuthenticodeCatalog {
    fn win_verify_trust_file(
        &self,
        path: &Path,
        policy: InstalledAuthenticodePolicy,
    ) -> Result<(), VerifyError>;
}

pub struct WinVerifyTrustCatalog;

impl AuthenticodeCatalog for WinVerifyTrustCatalog {
    fn win_verify_trust_file(
        &self,
        path: &Path,
        policy: InstalledAuthenticodePolicy,
    ) -> Result<(), VerifyError> {
        authenticode_win_verify_trust(path, policy)
    }
}

/// Frozen Windows Authenticode entry (`WINTRUST_ACTION_GENERIC_VERIFY_V2`).
/// This is not a raw RSA-over-bytes verifier.
pub fn authenticode_win_verify_trust(
    path: &Path,
    policy: InstalledAuthenticodePolicy,
) -> Result<(), VerifyError> {
    #[cfg(windows)]
    {
        win32::verify_file(path, policy)
    }
    #[cfg(not(windows))]
    {
        // Keep absent or unreadable artifacts distinct from the lack of a
        // platform WinVerifyTrust implementation.
        File::open(path).map_err(|_| VerifyError::Artifact)?;
        let _ = policy;
        Err(VerifyError::Authenticode)
    }
}

#[cfg(windows)]
mod win32;
pub fn verify_installed_authenticode(
    catalog: &impl AuthenticodeCatalog,
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
    pe_path: &Path,
) -> Result<VerifiedSignature, VerifyError> {
    let binding = bind_packet_artifact(packet, artifact_relative_path)?;
    // Keep this handle open through WinTrust. On Windows its share mode denies
    // replacement or mutation after the digest has bound the artifact bytes.
    let mut artifact = open_artifact_for_verification(pe_path)?;
    let observed = sha256_hex_file(&mut artifact, binding.artifact_byte_len)?;
    if !sha256_eq(binding.artifact_sha256(), &observed) {
        return Err(VerifyError::Artifact);
    }
    catalog.win_verify_trust_file(pe_path, INSTALLED_AUTHENTICODE_POLICY)?;
    Ok(VerifiedSignature { binding })
}

#[cfg(test)]
pub(crate) fn verify_installed_authenticode_for_observed_digest(
    catalog: &impl AuthenticodeCatalog,
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
    pe_path: &Path,
    observed_sha256: &str,
) -> Result<VerifiedSignature, VerifyError> {
    let binding = bind_packet_artifact(packet, artifact_relative_path)?;
    if !sha256_eq(binding.artifact_sha256(), observed_sha256) {
        return Err(VerifyError::Artifact);
    }
    catalog.win_verify_trust_file(pe_path, INSTALLED_AUTHENTICODE_POLICY)?;
    Ok(VerifiedSignature { binding })
}

fn open_artifact_for_verification(path: &Path) -> Result<File, VerifyError> {
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        std::fs::OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(path)
            .map_err(|_| VerifyError::Artifact)
    }
    #[cfg(not(windows))]
    {
        File::open(path).map_err(|_| VerifyError::Artifact)
    }
}

fn sha256_hex_file(file: &mut File, expected_byte_len: u64) -> Result<String, VerifyError> {
    let mut hasher = Sha256::new();
    let mut buffer = [0_u8; 64 * 1024];
    let mut remaining = expected_byte_len;
    while remaining != 0 {
        let chunk_len = remaining.min(buffer.len() as u64) as usize;
        let read = file
            .read(&mut buffer[..chunk_len])
            .map_err(|_| VerifyError::Artifact)?;
        if read == 0 {
            return Err(VerifyError::Artifact);
        }
        hasher.update(&buffer[..read]);
        remaining -= read as u64;
    }
    let mut trailing = [0_u8; 1];
    if file
        .read(&mut trailing)
        .map_err(|_| VerifyError::Artifact)?
        != 0
    {
        return Err(VerifyError::Artifact);
    }
    Ok(hex(&hasher.finalize()))
}

fn sha256_eq(claimed: &str, observed: &str) -> bool {
    claimed.len() == 64
        && claimed.len() == observed.len()
        && claimed
            .as_bytes()
            .iter()
            .zip(observed.as_bytes())
            .fold(0u8, |acc, (left, right)| acc | (left ^ right))
            == 0
}

fn hex(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(DIGITS[(byte >> 4) as usize] as char);
        value.push(DIGITS[(byte & 0x0f) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests;
