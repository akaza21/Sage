use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    installed_role::SignedInstalledRole,
    managed::ManagedStagingPrerequisite,
    manifest::{MetadataValidatedPacket, PINNED_ARTIFACT_COUNT, Platform},
    physical::{
        ConfiguredCodexSourceRoot, ExpectedPhysicalFile, InspectionClaims,
        PhysicallyInspectedPackage, decode_digest, inspect_claims, inspect_codex_source,
    },
    reproduction::PackageReproductionRequirements,
};
use crate::acp_protocol::codex_protocol::pinned_codex_version;

#[cfg(windows)]
mod windows;

mod digest;

pub(super) use digest::{hex, source_proof_digest};

mod helper;

#[cfg(all(windows, feature = "codex-provisioning-helper"))]
pub(crate) use helper::{
    HelperOperation, execute_boundary_publication, execute_boundary_recover,
    execute_boundary_remove, execute_boundary_update, helper_operation,
};

const SOURCE_FILE_COUNT: usize = PINNED_ARTIFACT_COUNT;
const SOURCE_BYTES_MAX: u64 = 1024 * 1024 * 1024;
const SOURCE_PATH_BYTES_MAX: usize = 32 * 1024;
const RECEIPT_BYTES_MAX: usize = 64 * 1024;
const STAGING_AUTH_ENV: &str = "SAGE_CODEX_STAGE_ONLY";
const STAGING_AUTH_VALUE: &str = "prepare-sealed-offline-stage-no-publish";
/// The sealed stage beside the managed package, scoped to the pinned release.
const STAGE_NAME: &str = concat!(".sage-codex-", pinned_codex_version!(), "-stage");

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ProvisioningError {
    #[error("Codex provisioning is unavailable on this platform")]
    PlatformUnavailable,
    #[error("the offline Codex source is not bound to the exact packet and reproduction facts")]
    SourceBinding,
    #[error("the offline Codex source path is invalid")]
    SourcePath,
    #[error("explicit offline staging authorization is absent")]
    AuthorizationRequired,
    #[error("a canonical Codex target already exists")]
    TargetExists,
    #[error("another or abandoned Codex staging transaction exists")]
    StageExists,
    #[error("the Codex staging namespace is malformed or has drifted")]
    StageDrift,
    #[error("the Codex staging copy exceeds a count, byte, depth, path, or time bound")]
    Bound,
    #[error("the Codex staging copy does not match the pinned identity")]
    CopyIdentity,
    #[error("the Codex staging receipt is malformed or does not match the sealed payload")]
    Receipt,
    #[error("the Codex staging transaction could not be synchronized durably")]
    Sync,
    #[error("the Codex staging transaction failed and rollback did not converge")]
    Rollback,
    #[error("package ACL finalization requires the separately reviewed privileged helper")]
    AclFinalizationUnavailable,
    #[error("atomic package/state publication is unavailable")]
    AtomicPublishUnavailable,
    #[error("Codex staging I/O failed")]
    Io,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum StagingOperation {
    Publish,
    Update,
}

#[derive(Debug, Clone)]
pub(super) struct SourceFile {
    relative_path: PathBuf,
    sha256: [u8; 32],
    sha256_hex: String,
    byte_len: u64,
}

#[derive(Debug)]
enum SourceIdentity {
    Production(Box<MetadataValidatedPacket>),
    #[cfg(test)]
    Fixture,
}

/// A retained, non-forgeable proof that an offline source tree contains only
/// the exact pinned Codex closure. It grants no staging, ACL, publish,
/// catalog, admission, or launch authority.
#[derive(Debug)]
pub(crate) struct VerifiedCodexSource {
    identity: SourceIdentity,
    inspection: PhysicallyInspectedPackage,
    files: Vec<SourceFile>,
    package_target: PathBuf,
    state_target: PathBuf,
    source_proof_sha256: String,
}

impl VerifiedCodexSource {
    pub(super) fn source_root(&self) -> &Path {
        self.inspection.package_root()
    }

    fn inspect_root(
        &self,
        root: PathBuf,
        state: PathBuf,
    ) -> Result<PhysicallyInspectedPackage, ProvisioningError> {
        match &self.identity {
            SourceIdentity::Production(packet) if root == self.source_root() => {
                let root = ConfiguredCodexSourceRoot::new(root)
                    .map_err(|_| ProvisioningError::SourcePath)?;
                inspect_codex_source(packet, &root).map_err(|_| ProvisioningError::SourceBinding)
            }
            SourceIdentity::Production(_) => inspect_claims(InspectionClaims {
                package_root: root,
                codex_home: state,
                files: self.expected_files(),
                aggregate_bytes: self.inspection.byte_len(),
            })
            .map_err(|_| ProvisioningError::StageDrift),
            #[cfg(test)]
            SourceIdentity::Fixture => inspect_claims(InspectionClaims {
                package_root: root,
                codex_home: state,
                files: self.expected_files(),
                aggregate_bytes: self.inspection.byte_len(),
            })
            .map_err(|_| ProvisioningError::StageDrift),
        }
    }

    pub(super) fn revalidate(&self) -> Result<(), ProvisioningError> {
        let fresh = self.inspect_root(self.source_root().to_owned(), self.state_target.clone())?;
        if fresh.package_root() != self.inspection.package_root()
            || fresh.artifact_count() != self.inspection.artifact_count()
            || fresh.byte_len() != self.inspection.byte_len()
        {
            return Err(ProvisioningError::SourceBinding);
        }
        Ok(())
    }

    fn expected_files(&self) -> Vec<ExpectedPhysicalFile> {
        self.files
            .iter()
            .map(|file| ExpectedPhysicalFile {
                relative_path: file.relative_path.clone(),
                sha256: file.sha256,
                byte_len: file.byte_len,
            })
            .collect()
    }

    fn admits_signed_update(&self, role: &SignedInstalledRole) -> bool {
        match &self.identity {
            SourceIdentity::Production(packet) => packet
                .physical_artifact_claims()
                .next()
                .is_some_and(|(_, artifact)| {
                    role.binding_id == packet.metadata().binding_id
                        && role.artifact_relative_path == artifact
                }),
            #[cfg(test)]
            SourceIdentity::Fixture => false,
        }
    }

    #[cfg(test)]
    fn fixture(
        source_root: PathBuf,
        package_target: PathBuf,
        state_target: PathBuf,
        files: Vec<(PathBuf, Vec<u8>)>,
    ) -> Result<Self, ProvisioningError> {
        let source_files = files
            .into_iter()
            .map(|(relative_path, bytes)| {
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                SourceFile {
                    relative_path,
                    sha256: digest,
                    sha256_hex: hex(&digest),
                    byte_len: bytes.len() as u64,
                }
            })
            .collect::<Vec<_>>();
        let inspection = inspect_claims(InspectionClaims {
            package_root: source_root,
            codex_home: state_target.clone(),
            files: source_files
                .iter()
                .map(|file| ExpectedPhysicalFile {
                    relative_path: file.relative_path.clone(),
                    sha256: file.sha256,
                    byte_len: file.byte_len,
                })
                .collect(),
            aggregate_bytes: source_files.iter().map(|file| file.byte_len).sum(),
        })
        .map_err(|_| ProvisioningError::SourceBinding)?;
        verified_source(
            SourceIdentity::Fixture,
            inspection,
            source_files,
            package_target,
            state_target,
        )
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HandoffFile {
    relative_path: PathBuf,
    sha256: String,
    byte_len: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct AclPlan {
    owner: String,
    protected_dacl: bool,
    system_access_mask: u32,
    administrators_access_mask: u32,
    current_user_access_mask: u32,
    inheritance_flags: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct HandoffPayload {
    schema_version: u8,
    binding_id: String,
    runtime_version: String,
    creation_nonce: String,
    source_proof_sha256: String,
    aggregate_byte_len: u64,
    files: Vec<HandoffFile>,
    package_target: PathBuf,
    state_target: PathBuf,
    current_user_sid_hex: String,
    package_acl: AclPlan,
    state_acl: AclPlan,
    acl_application_status: String,
    publish_status: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct SealedHandoff {
    payload: HandoffPayload,
    payload_sha256: String,
}

#[derive(Debug)]
pub(crate) struct ExplicitStagingAuthorization(());

pub(crate) fn authorize_offline_staging_from_environment()
-> Result<ExplicitStagingAuthorization, ProvisioningError> {
    match std::env::var(STAGING_AUTH_ENV) {
        Ok(value) if value == STAGING_AUTH_VALUE => Ok(ExplicitStagingAuthorization(())),
        _ => Err(ProvisioningError::AuthorizationRequired),
    }
}

#[cfg(test)]
fn fixture_authorization() -> ExplicitStagingAuthorization {
    ExplicitStagingAuthorization(())
}

#[derive(Debug)]
pub(crate) struct PreparedCodexStaging {
    #[cfg(windows)]
    inner: windows::WindowsPreparedStaging,
}

impl PreparedCodexStaging {
    pub(crate) fn revalidate(&self) -> Result<(), ProvisioningError> {
        #[cfg(windows)]
        {
            self.inner.revalidate()
        }
        #[cfg(not(windows))]
        {
            Err(ProvisioningError::PlatformUnavailable)
        }
    }

    pub(crate) fn stage_root(&self) -> &Path {
        #[cfg(windows)]
        {
            self.inner.stage_root()
        }
        #[cfg(not(windows))]
        {
            Path::new("")
        }
    }

    pub(crate) fn publish(self) -> Result<ManagedStagingPrerequisite, ProvisioningError> {
        Err(ProvisioningError::AclFinalizationUnavailable)
    }

    #[cfg(all(test, windows))]
    fn corrupt_receipt_for_test(&mut self) {
        self.inner.corrupt_receipt_for_test();
    }
}

pub(crate) fn prepare_codex_staging(
    source: &VerifiedCodexSource,
    authorization: &ExplicitStagingAuthorization,
) -> Result<PreparedCodexStaging, ProvisioningError> {
    let _ = authorization;
    source.revalidate()?;
    #[cfg(windows)]
    {
        windows::prepare(source).map(|inner| PreparedCodexStaging { inner })
    }
    #[cfg(not(windows))]
    {
        Err(ProvisioningError::PlatformUnavailable)
    }
}

/// Stages a replacement only when the caller presents the installed role that
/// was authenticated against this exact source packet. Existing canonical
/// targets therefore select the typed Update helper path instead of becoming a
/// host-side overwrite or an unauthenticated retry.
pub(crate) fn prepare_codex_staging_after_signed_role(
    source: &VerifiedCodexSource,
    authorization: &ExplicitStagingAuthorization,
    role: &SignedInstalledRole,
) -> Result<(PreparedCodexStaging, StagingOperation), ProvisioningError> {
    let _ = authorization;
    source.revalidate()?;
    if !source.admits_signed_update(role) {
        return Err(ProvisioningError::AuthorizationRequired);
    }
    #[cfg(windows)]
    {
        let operation = windows::target_operation(source)?;
        windows::prepare_for_operation(source, operation)
            .map(|inner| (PreparedCodexStaging { inner }, operation))
    }
    #[cfg(not(windows))]
    {
        Err(ProvisioningError::PlatformUnavailable)
    }
}

pub(crate) fn verify_codex_source(
    requirements: PackageReproductionRequirements,
    packet: &MetadataValidatedPacket,
    source_root: PathBuf,
) -> Result<VerifiedCodexSource, ProvisioningError> {
    if packet.metadata().platform != Platform::WindowsX86_64 {
        return Err(ProvisioningError::PlatformUnavailable);
    }
    requirements
        .revalidate(packet)
        .map_err(|_| ProvisioningError::SourceBinding)?;
    if source_root.as_os_str().len() > SOURCE_PATH_BYTES_MAX {
        return Err(ProvisioningError::SourcePath);
    }
    let root =
        ConfiguredCodexSourceRoot::new(source_root).map_err(|_| ProvisioningError::SourcePath)?;
    let inspection =
        inspect_codex_source(packet, &root).map_err(|_| ProvisioningError::SourceBinding)?;
    let files = packet
        .physical_artifact_claims()
        .map(|(artifact, relative)| {
            Ok(SourceFile {
                relative_path: PathBuf::from(relative),
                sha256: decode_digest(&artifact.sha256)
                    .map_err(|_| ProvisioningError::SourceBinding)?,
                sha256_hex: artifact.sha256.clone(),
                byte_len: artifact.byte_len,
            })
        })
        .collect::<Result<Vec<_>, ProvisioningError>>()?;
    verified_source(
        SourceIdentity::Production(Box::new(packet.clone())),
        inspection,
        files,
        packet.managed_package_root(),
        packet.codex_home().to_owned(),
    )
}

fn verified_source(
    identity: SourceIdentity,
    inspection: PhysicallyInspectedPackage,
    files: Vec<SourceFile>,
    package_target: PathBuf,
    state_target: PathBuf,
) -> Result<VerifiedCodexSource, ProvisioningError> {
    if files.len() != SOURCE_FILE_COUNT
        || inspection.artifact_count() != SOURCE_FILE_COUNT
        || inspection.byte_len() == 0
        || inspection.byte_len() > SOURCE_BYTES_MAX
    {
        return Err(ProvisioningError::SourceBinding);
    }
    let source_proof_sha256 = source_proof_digest(inspection.package_root(), &files);
    Ok(VerifiedCodexSource {
        identity,
        inspection,
        files,
        package_target,
        state_target,
        source_proof_sha256,
    })
}

#[cfg(all(test, windows))]
mod tests;
