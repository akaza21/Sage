use serde::Deserialize;

use super::manifest::{
    AUTHENTICODE_SIGNED_FILES, AUTHENTICODE_UNSIGNED_FILES, MetadataValidatedPacket, Platform,
};
use super::physical::PhysicallyInspectedPackage;
use sage_acp_protocol::pinned_codex_version;

pub(super) const REPRODUCTION_BYTES: usize = 32 * 1024;

const TARBALL_URL: &str = concat!(
    "https://registry.npmjs.org/@openai/codex/-/codex-",
    pinned_codex_version!(),
    "-win32-x64.tgz"
);
const TARBALL_SHA512_HEX: &str = "76e3c20707e573caa141d2de91b2fc7c3605eb8ead5b8823d3f97d0439ec561133a2a9ba3b0c4c0095c317048e2d45093c29a305e33e787600c787cffe1c1f8e";
const NPM_SIGNATURE: &str = "MEQCIBQPw3rcviPvLyJGEYjJ/zkwb4orLwRUhKk315KRztE/AiBjRD1eKIfzicLQ0TrqPCdkdKaL4esvjfiJWbSM4/15fg==";
const NPM_KEY_ID: &str = "SHA256:DhQ8wR5APBvFHLF/+Tc+AYvPOdTpcIDqOhxsBHRwC7U";
const ATTESTATION_URL: &str = concat!(
    "https://registry.npmjs.org/-/npm/v1/attestations/@openai%2fcodex@",
    pinned_codex_version!(),
    "-win32-x64"
);
const SLSA_PREDICATE: &str = "https://slsa.dev/provenance/v1";
const OPENAI_PUBLISHER: &str = "OpenAI OpCo, LLC";
/// SHA-1 thumbprint of the publisher certificate that signed the pinned
/// release's files.
const OPENAI_THUMBPRINT: &str = "DE62726DB667B59DB0FD14B38F858D16C96804A0";

pub(super) fn authenticode_publisher_common_name() -> &'static str {
    OPENAI_PUBLISHER
}

pub(super) fn authenticode_certificate_thumbprint_sha1() -> &'static str {
    OPENAI_THUMBPRINT
}

pub(super) fn authenticode_signed_relative_paths() -> &'static [&'static str] {
    &AUTHENTICODE_SIGNED_FILES
}

pub(super) fn authenticode_unsigned_relative_paths() -> &'static [&'static str] {
    &AUTHENTICODE_UNSIGNED_FILES
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct PackageReproductionRequirements {
    schema_version: u8,
    binding_id: String,
    platform: Platform,
    codex_source_commit: String,
    codex_runtime_version: String,
    package_name: String,
    package_version: String,
    registry: RegistryRequirements,
    package_metadata: PackageMetadataRequirements,
    authenticode_observation: AuthenticodeObservation,
    managed_staging: ManagedStagingRequirements,
}

impl PackageReproductionRequirements {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
    ) -> Result<(), ReproductionError> {
        validate_requirements(self, packet)
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct RegistryRequirements {
    tarball_url: String,
    tarball_sha1: String,
    tarball_integrity_sha512: String,
    tarball_sha512_hex: String,
    packed_byte_len: u64,
    unpacked_byte_len: u64,
    file_count: u8,
    npm_signature: NpmSignatureObservation,
    slsa_provenance: SlsaProvenanceObservation,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct NpmSignatureObservation {
    signature_base64: String,
    key_id: String,
    verification_status: RegistryVerificationStatus,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SlsaProvenanceObservation {
    attestation_url: String,
    predicate_type: String,
    verification_status: RegistryVerificationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum RegistryVerificationStatus {
    PresenceRecordedUnverified,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct PackageMetadataRequirements {
    declared_license: String,
    license_file: PackageFileStatus,
    notice_file: PackageFileStatus,
    license_closure_status: LicenseClosureStatus,
    sbom_status: SbomStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PackageFileStatus {
    Absent,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum LicenseClosureStatus {
    Incomplete,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum SbomStatus {
    Unavailable,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct AuthenticodeObservation {
    verification_status: AuthenticodeVerificationStatus,
    publisher_common_name: String,
    certificate_thumbprint_sha1: String,
    signed_files: Vec<String>,
    unsigned_files: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum AuthenticodeVerificationStatus {
    WorkstationObservationOnly,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct ManagedStagingRequirements {
    physical_inspection: PhysicalInspectionStatus,
    acl_ownership: IndependentVerificationStatus,
    immutability: IndependentVerificationStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum PhysicalInspectionStatus {
    RequiredAtRuntimeConstruction,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
enum IndependentVerificationStatus {
    Unverified,
    #[serde(other)]
    Unsupported,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ReproductionError {
    #[error("Codex reproduction requirements exceed their byte limit")]
    EvidenceBytes,
    #[error("Codex reproduction requirements are malformed or have an unknown field")]
    Malformed,
    #[error("Codex reproduction identity does not match the metadata packet")]
    Identity,
    #[error("Codex registry reproduction facts do not match the pinned package")]
    Registry,
    #[error("Codex independent-verification boundary is overstated or has drifted")]
    VerificationBoundary,
    #[error("physical Codex package inspection is not bound to this packet")]
    PhysicalBinding,
}

/// This remains a non-authoritative prerequisite: it neither upgrades
/// workstation signature observations nor grants artifact admission or launch
/// authority.
#[derive(Debug)]
pub(crate) struct ReproducedPackagePrerequisite {
    requirements: PackageReproductionRequirements,
    inspection: PhysicallyInspectedPackage,
}

impl ReproducedPackagePrerequisite {
    pub(super) fn revalidate(
        &self,
        packet: &MetadataValidatedPacket,
    ) -> Result<(), ReproductionError> {
        validate_requirements(&self.requirements, packet)?;
        if !self.inspection.binds_packet(packet) {
            return Err(ReproductionError::PhysicalBinding);
        }
        Ok(())
    }

    pub(super) fn inspection(&self) -> &PhysicallyInspectedPackage {
        &self.inspection
    }

    #[cfg(test)]
    pub(super) fn fixture(packet: &MetadataValidatedPacket) -> Self {
        let requirements = parse_reproduction_requirements(
            include_bytes!("../../../../testdata/acp/codex/package-reproduction-v1.json"),
            packet,
        )
        .expect("pinned reproduction requirements");
        Self {
            requirements,
            inspection: PhysicallyInspectedPackage::matching_fixture(packet),
        }
    }
}

pub(crate) fn bind_reproduced_package(
    requirements: PackageReproductionRequirements,
    packet: &MetadataValidatedPacket,
    inspection: PhysicallyInspectedPackage,
) -> Result<ReproducedPackagePrerequisite, ReproductionError> {
    validate_requirements(&requirements, packet)?;
    if !inspection.binds_packet(packet) {
        return Err(ReproductionError::PhysicalBinding);
    }
    Ok(ReproducedPackagePrerequisite {
        requirements,
        inspection,
    })
}

pub(crate) fn parse_reproduction_requirements(
    bytes: &[u8],
    packet: &MetadataValidatedPacket,
) -> Result<PackageReproductionRequirements, ReproductionError> {
    if bytes.len() > REPRODUCTION_BYTES {
        return Err(ReproductionError::EvidenceBytes);
    }
    let requirements = serde_json::from_slice(bytes).map_err(|_| ReproductionError::Malformed)?;
    validate_requirements(&requirements, packet)?;
    Ok(requirements)
}

fn validate_requirements(
    requirements: &PackageReproductionRequirements,
    packet: &MetadataValidatedPacket,
) -> Result<(), ReproductionError> {
    let metadata = packet.metadata();
    if requirements.schema_version != 1
        || requirements.binding_id != metadata.binding_id
        || requirements.platform != metadata.platform
        || requirements.codex_source_commit != metadata.codex_source_commit
        || requirements.codex_runtime_version != metadata.codex_runtime_version
        || requirements.package_name != metadata.package_name
        || requirements.package_version != metadata.package_version
    {
        return Err(ReproductionError::Identity);
    }

    let registry = &requirements.registry;
    if registry.tarball_url != TARBALL_URL
        || registry.tarball_sha1 != metadata.package_tarball_sha1
        || registry.tarball_integrity_sha512 != metadata.package_integrity_sha512
        || registry.tarball_sha512_hex != TARBALL_SHA512_HEX
        || registry.packed_byte_len != metadata.package_packed_byte_len
        || registry.unpacked_byte_len != metadata.package_unpacked_byte_len
        || registry.file_count as usize != metadata.supporting_artifacts.len() + 1
        || registry.npm_signature.signature_base64 != NPM_SIGNATURE
        || registry.npm_signature.key_id != NPM_KEY_ID
        || registry.slsa_provenance.attestation_url != ATTESTATION_URL
        || registry.slsa_provenance.predicate_type != SLSA_PREDICATE
        || requirements.authenticode_observation.publisher_common_name != OPENAI_PUBLISHER
        || requirements
            .authenticode_observation
            .certificate_thumbprint_sha1
            != OPENAI_THUMBPRINT
        || requirements.authenticode_observation.signed_files != AUTHENTICODE_SIGNED_FILES
        || requirements.authenticode_observation.unsigned_files != AUTHENTICODE_UNSIGNED_FILES
        || requirements.package_metadata.declared_license != "Apache-2.0"
    {
        return Err(ReproductionError::Registry);
    }

    if registry.npm_signature.verification_status
        != RegistryVerificationStatus::PresenceRecordedUnverified
        || registry.slsa_provenance.verification_status
            != RegistryVerificationStatus::PresenceRecordedUnverified
        || requirements.package_metadata.license_file != PackageFileStatus::Absent
        || requirements.package_metadata.notice_file != PackageFileStatus::Absent
        || requirements.package_metadata.license_closure_status != LicenseClosureStatus::Incomplete
        || requirements.package_metadata.sbom_status != SbomStatus::Unavailable
        || requirements.authenticode_observation.verification_status
            != AuthenticodeVerificationStatus::WorkstationObservationOnly
        || requirements.managed_staging.physical_inspection
            != PhysicalInspectionStatus::RequiredAtRuntimeConstruction
        || requirements.managed_staging.acl_ownership != IndependentVerificationStatus::Unverified
        || requirements.managed_staging.immutability != IndependentVerificationStatus::Unverified
    {
        return Err(ReproductionError::VerificationBoundary);
    }
    Ok(())
}
