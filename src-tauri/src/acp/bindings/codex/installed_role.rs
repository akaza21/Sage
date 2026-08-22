use super::manifest::MetadataValidatedPacket;
use super::trust_root::VerifiedSignature;

#[derive(Debug, PartialEq, Eq)]
pub struct SignedInstalledRole {
    pub(crate) binding_id: String,
    pub(crate) artifact_relative_path: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AdmissionError {
    #[error("verified signature is not bound to this packet")]
    Unbound,
}

pub fn admit_installed_role(
    packet: &MetadataValidatedPacket,
    signature: VerifiedSignature,
) -> Result<SignedInstalledRole, AdmissionError> {
    if signature.binding_id() != packet.metadata().binding_id {
        return Err(AdmissionError::Unbound);
    }
    Ok(SignedInstalledRole {
        binding_id: signature.binding_id().to_owned(),
        artifact_relative_path: signature.artifact_relative_path().to_owned(),
    })
}

#[cfg(test)]
#[path = "installed_role/tests.rs"]
mod tests;
