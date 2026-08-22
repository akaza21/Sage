use std::path::Path;

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
    catalog.win_verify_trust_file(pe_path, INSTALLED_AUTHENTICODE_POLICY)?;
    Ok(VerifiedSignature { binding })
}

#[cfg(test)]
#[path = "trust_root/tests.rs"]
mod tests;
