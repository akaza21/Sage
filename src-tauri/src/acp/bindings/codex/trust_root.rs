use std::path::Path;

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
    let _ = (
        path,
        policy.publisher_common_name(),
        policy.certificate_thumbprint_sha1(),
    );
    Err(VerifyError::Authenticode)
}

pub fn verify_installed_authenticode(
    catalog: &impl AuthenticodeCatalog,
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
    pe_path: &Path,
) -> Result<VerifiedSignature, VerifyError> {
    let binding = bind_packet_artifact(packet, artifact_relative_path)?;
    let observed = sha256_hex_file(pe_path)?;
    finish_verified_signature(catalog, binding, pe_path, &observed)
}

pub(crate) fn verify_installed_authenticode_for_observed_digest(
    catalog: &impl AuthenticodeCatalog,
    packet: &MetadataValidatedPacket,
    artifact_relative_path: &str,
    pe_path: &Path,
    observed_sha256: &str,
) -> Result<VerifiedSignature, VerifyError> {
    let binding = bind_packet_artifact(packet, artifact_relative_path)?;
    finish_verified_signature(catalog, binding, pe_path, observed_sha256)
}

fn finish_verified_signature(
    catalog: &impl AuthenticodeCatalog,
    binding: PacketArtifactBinding,
    pe_path: &Path,
    observed_sha256: &str,
) -> Result<VerifiedSignature, VerifyError> {
    if !sha256_eq(binding.artifact_sha256(), observed_sha256) {
        return Err(VerifyError::Artifact);
    }
    catalog.win_verify_trust_file(pe_path, INSTALLED_AUTHENTICODE_POLICY)?;
    Ok(VerifiedSignature { binding })
}

fn sha256_hex_file(path: &Path) -> Result<String, VerifyError> {
    let bytes = std::fs::read(path).map_err(|_| VerifyError::Artifact)?;
    Ok(hex(&Sha256::digest(bytes)))
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
#[path = "trust_root/tests.rs"]
mod tests;
