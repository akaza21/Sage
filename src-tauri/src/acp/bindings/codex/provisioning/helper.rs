use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{AclPlan, SealedHandoff, hex};
use crate::acp::bindings::codex::{
    managed::ManagedStagingPrerequisite, manifest::MetadataValidatedPacket,
};

#[path = "helper/transaction.rs"]
mod transaction;

#[cfg(windows)]
#[path = "helper/windows.rs"]
mod windows;

#[cfg(all(windows, feature = "codex-provisioning-helper"))]
#[path = "helper/boundary.rs"]
mod boundary;

#[cfg(all(windows, feature = "codex-provisioning-helper"))]
pub(crate) use boundary::{
    execute_boundary_publication, execute_boundary_recover, execute_boundary_remove,
    execute_boundary_update,
};

const REQUEST_BYTES_MAX: usize = 128 * 1024;
const HELPER_EXECUTABLE_BYTES_MAX: u64 = 32 * 1024 * 1024;
const HELPER_PRODUCT: &str = "sage-codex-0145-elevated-helper";
const HELPER_PROTOCOL_VERSION: u8 = 1;
const REQUEST_SCHEMA_VERSION: u8 = 1;
const PACKAGE_OWNER: &str = "builtinAdministrators";
const STATE_OWNER: &str = "currentUser";
const FILE_ALL_ACCESS_MASK: u32 = 0x001f_01ff;
const FILE_READ_EXECUTE_MASK: u32 = 0x0012_00a9;
const INHERIT_FILES_AND_DIRECTORIES: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub(crate) enum ElevatedHelperError {
    #[error("the Codex elevated-helper request exceeds its byte limit")]
    RequestBytes,
    #[error("the Codex elevated-helper request is malformed")]
    Malformed,
    #[error("the Codex elevated-helper authenticator does not match")]
    Authentication,
    #[error("the Codex elevated-helper executable identity does not match")]
    HelperIdentity,
    #[error("the Codex elevated-helper handoff identity does not match")]
    HandoffIdentity,
    #[error("the Codex elevated-helper ACL plan does not match")]
    AclPlan,
    #[error("the Codex elevated-helper recovery journal is invalid or exceeds its limit")]
    Journal,
    #[error("the Codex elevated-helper namespace collides with another transaction")]
    Collision,
    #[error("the Codex elevated-helper observed namespace or held identity drifted")]
    Drift,
    #[error("the Codex elevated-helper could not apply the exact ACL plan")]
    AclApplication,
    #[error("the Codex elevated-helper package publication failed")]
    Publication,
    #[error("the Codex elevated-helper final managed proof failed")]
    FinalProof,
    #[error("the Codex elevated-helper rollback did not converge")]
    Rollback,
    #[error("the Codex elevated-helper committed but cleanup must be retried")]
    Cleanup,
    #[error("production elevation remains unavailable until the exact helper boundary is admitted")]
    ElevationBoundaryUnavailable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperExecutableIdentity {
    product: String,
    protocol_version: u8,
    sage_version: String,
    executable_sha256: String,
    executable_byte_len: u64,
}

#[derive(Debug)]
pub(crate) struct VerifiedHelperExecutable(HelperExecutableIdentity);

impl VerifiedHelperExecutable {
    #[cfg(test)]
    fn fixture(sage_version: &str, sha256: [u8; 32], byte_len: u64) -> Self {
        Self(HelperExecutableIdentity {
            product: HELPER_PRODUCT.into(),
            protocol_version: HELPER_PROTOCOL_VERSION,
            sage_version: sage_version.into(),
            executable_sha256: hex(&sha256),
            executable_byte_len: byte_len,
        })
    }
}

pub(crate) struct OneTimeHelperSecret([u8; 32]);

impl OneTimeHelperSecret {
    #[cfg(test)]
    fn fixture(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl Drop for OneTimeHelperSecret {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperRequestPayload {
    schema_version: u8,
    #[serde(default)]
    operation: HelperOperation,
    stage_root: PathBuf,
    helper: HelperExecutableIdentity,
    handoff: SealedHandoff,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub(crate) enum HelperOperation {
    #[default]
    Publish,
    Recover,
    Update,
    Remove,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HelperRequestEnvelope {
    payload: HelperRequestPayload,
    authenticator_sha256: String,
}

pub(crate) struct AuthenticatedHelperRequest {
    payload: HelperRequestPayload,
    journal_key: Vec<u8>,
}

impl std::fmt::Debug for AuthenticatedHelperRequest {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("AuthenticatedHelperRequest([redacted])")
    }
}

impl AuthenticatedHelperRequest {
    pub(crate) fn creation_nonce(&self) -> &str {
        &self.payload.handoff.payload.creation_nonce
    }

    fn stage_root(&self) -> &Path {
        &self.payload.stage_root
    }

    fn package_target(&self) -> &Path {
        &self.payload.handoff.payload.package_target
    }

    fn state_target(&self) -> &Path {
        &self.payload.handoff.payload.state_target
    }

    fn binding_sha256(&self) -> Result<String, ElevatedHelperError> {
        let bytes = self.journal_binding()?;
        Ok(hex(&Sha256::digest(bytes)))
    }

    fn journal_authenticator(&self, bytes: &[u8]) -> String {
        let mut authenticated = Vec::with_capacity(30 + bytes.len());
        authenticated.extend_from_slice(b"sage-codex-helper-journal-v1\0");
        authenticated.extend_from_slice(bytes);
        hex(&hmac_sha256(&self.journal_key, &authenticated))
    }

    fn journal_binding(&self) -> Result<Vec<u8>, ElevatedHelperError> {
        let helper =
            serde_json::to_vec(&self.payload.helper).map_err(|_| ElevatedHelperError::Malformed)?;
        let handoff = serde_json::to_vec(&self.payload.handoff)
            .map_err(|_| ElevatedHelperError::Malformed)?;
        let mut binding = Vec::with_capacity(helper.len() + handoff.len() + 1);
        binding.extend_from_slice(&helper);
        binding.push(0);
        binding.extend_from_slice(&handoff);
        Ok(binding)
    }

    fn operation(&self) -> HelperOperation {
        self.payload.operation
    }
}

pub(crate) fn seal_helper_request(
    handoff: &SealedHandoff,
    stage_root: PathBuf,
    helper: &VerifiedHelperExecutable,
    secret: &OneTimeHelperSecret,
) -> Result<Vec<u8>, ElevatedHelperError> {
    seal_helper_request_with_secret(handoff, stage_root, helper, &secret.0)
}

fn seal_helper_request_with_secret(
    handoff: &SealedHandoff,
    stage_root: PathBuf,
    helper: &VerifiedHelperExecutable,
    secret: &[u8; 32],
) -> Result<Vec<u8>, ElevatedHelperError> {
    seal_helper_request_operation_with_secret(
        handoff,
        stage_root,
        helper,
        secret,
        HelperOperation::Publish,
    )
}

fn seal_helper_request_operation_with_secret(
    handoff: &SealedHandoff,
    stage_root: PathBuf,
    helper: &VerifiedHelperExecutable,
    secret: &[u8; 32],
    operation: HelperOperation,
) -> Result<Vec<u8>, ElevatedHelperError> {
    let payload = HelperRequestPayload {
        schema_version: REQUEST_SCHEMA_VERSION,
        operation,
        stage_root,
        helper: helper.0.clone(),
        handoff: handoff.clone(),
    };
    let payload_bytes = serde_json::to_vec(&payload).map_err(|_| ElevatedHelperError::Malformed)?;
    let envelope = HelperRequestEnvelope {
        authenticator_sha256: hex(&hmac_sha256(secret, &payload_bytes)),
        payload,
    };
    let bytes = serde_json::to_vec(&envelope).map_err(|_| ElevatedHelperError::Malformed)?;
    if bytes.len() > REQUEST_BYTES_MAX {
        return Err(ElevatedHelperError::RequestBytes);
    }
    Ok(bytes)
}

pub(crate) fn helper_operation(bytes: &[u8]) -> Result<HelperOperation, ElevatedHelperError> {
    if bytes.is_empty() || bytes.len() > REQUEST_BYTES_MAX {
        return Err(ElevatedHelperError::RequestBytes);
    }
    serde_json::from_slice::<HelperRequestEnvelope>(bytes)
        .map(|envelope| envelope.payload.operation)
        .map_err(|_| ElevatedHelperError::Malformed)
}

pub(crate) fn authenticate_helper_request(
    bytes: &[u8],
    secret: &OneTimeHelperSecret,
    observed_helper: &VerifiedHelperExecutable,
    packet: &MetadataValidatedPacket,
) -> Result<AuthenticatedHelperRequest, ElevatedHelperError> {
    authenticate_helper_request_with_secret(bytes, &secret.0, observed_helper, packet)
}

fn authenticate_helper_request_with_secret(
    bytes: &[u8],
    secret: &[u8; 32],
    observed_helper: &VerifiedHelperExecutable,
    packet: &MetadataValidatedPacket,
) -> Result<AuthenticatedHelperRequest, ElevatedHelperError> {
    if bytes.is_empty() || bytes.len() > REQUEST_BYTES_MAX {
        return Err(ElevatedHelperError::RequestBytes);
    }
    let envelope: HelperRequestEnvelope =
        serde_json::from_slice(bytes).map_err(|_| ElevatedHelperError::Malformed)?;
    let payload_bytes =
        serde_json::to_vec(&envelope.payload).map_err(|_| ElevatedHelperError::Malformed)?;
    let expected = hex(&hmac_sha256(secret, &payload_bytes));
    if !constant_time_equal(
        expected.as_bytes(),
        envelope.authenticator_sha256.as_bytes(),
    ) {
        return Err(ElevatedHelperError::Authentication);
    }
    validate_helper_identity(&envelope.payload.helper, observed_helper)?;
    validate_handoff(&envelope.payload, packet)?;
    let request = AuthenticatedHelperRequest {
        payload: envelope.payload,
        journal_key: Vec::new(),
    };
    let journal_key = request.journal_binding()?;
    Ok(AuthenticatedHelperRequest {
        payload: request.payload,
        journal_key,
    })
}

pub(crate) fn request_production_elevation(
    _request: AuthenticatedHelperRequest,
) -> Result<ManagedStagingPrerequisite, ElevatedHelperError> {
    Err(ElevatedHelperError::ElevationBoundaryUnavailable)
}

fn validate_helper_identity(
    declared: &HelperExecutableIdentity,
    observed: &VerifiedHelperExecutable,
) -> Result<(), ElevatedHelperError> {
    if declared != &observed.0
        || declared.product != HELPER_PRODUCT
        || declared.protocol_version != HELPER_PROTOCOL_VERSION
        || declared.sage_version != env!("CARGO_PKG_VERSION")
        || declared.executable_byte_len == 0
        || declared.executable_byte_len > HELPER_EXECUTABLE_BYTES_MAX
        || !lower_hex(&declared.executable_sha256, 64)
    {
        return Err(ElevatedHelperError::HelperIdentity);
    }
    Ok(())
}

fn validate_handoff(
    request: &HelperRequestPayload,
    packet: &MetadataValidatedPacket,
) -> Result<(), ElevatedHelperError> {
    let handoff = &request.handoff;
    let payload_bytes =
        serde_json::to_vec(&handoff.payload).map_err(|_| ElevatedHelperError::Malformed)?;
    if handoff.payload_sha256 != hex(&Sha256::digest(payload_bytes)) {
        return Err(ElevatedHelperError::Authentication);
    }
    let payload = &handoff.payload;
    let expected_stage = packet
        .managed_package_root()
        .parent()
        .ok_or(ElevatedHelperError::HandoffIdentity)?
        .join(".sage-codex-0.145.0-stage");
    let expected_files = packet.physical_artifact_claims().collect::<Vec<_>>();
    if request.schema_version != REQUEST_SCHEMA_VERSION
        || request.stage_root != expected_stage
        || payload.schema_version != 1
        || payload.binding_id != "codex-app-server"
        || payload.runtime_version != "0.145.0"
        || !lower_hex(&payload.creation_nonce, 32)
        || !lower_hex(&payload.source_proof_sha256, 64)
        || !lower_even_hex(&payload.current_user_sid_hex, 16, 136)
        || payload.aggregate_byte_len != packet.metadata().package_unpacked_byte_len
        || payload.package_target != packet.managed_package_root()
        || payload.state_target != packet.codex_home()
        || payload.files.len() != 8
        || payload.files.len() != expected_files.len()
        || payload
            .files
            .iter()
            .zip(expected_files)
            .any(|(file, (artifact, relative))| {
                file.relative_path != Path::new(relative)
                    || file.sha256 != artifact.sha256
                    || file.byte_len != artifact.byte_len
            })
        || payload.acl_application_status != "requiredPrivilegedHelperNotApplied"
        || payload.publish_status != "notPublished"
    {
        return Err(ElevatedHelperError::HandoffIdentity);
    }
    validate_acl_plan(&payload.package_acl, true)?;
    validate_acl_plan(&payload.state_acl, false)
}

fn validate_acl_plan(plan: &AclPlan, package: bool) -> Result<(), ElevatedHelperError> {
    let valid = plan.owner == if package { PACKAGE_OWNER } else { STATE_OWNER }
        && plan.protected_dacl
        && plan.system_access_mask == FILE_ALL_ACCESS_MASK
        && plan.administrators_access_mask == FILE_ALL_ACCESS_MASK
        && plan.current_user_access_mask
            == if package {
                FILE_READ_EXECUTE_MASK
            } else {
                FILE_ALL_ACCESS_MASK
            }
        && plan.inheritance_flags == INHERIT_FILES_AND_DIRECTORIES;
    if !valid {
        return Err(ElevatedHelperError::AclPlan);
    }
    Ok(())
}

fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK_BYTES: usize = 64;
    let mut inner_key = [0x36_u8; BLOCK_BYTES];
    let mut outer_key = [0x5c_u8; BLOCK_BYTES];
    let hashed_key;
    let key = if key.len() > BLOCK_BYTES {
        hashed_key = Sha256::digest(key);
        hashed_key.as_slice()
    } else {
        key
    };
    for (index, byte) in key.iter().enumerate() {
        inner_key[index] ^= byte;
        outer_key[index] ^= byte;
    }
    let mut inner = Sha256::new();
    inner.update(inner_key);
    inner.update(message);
    let mut outer = Sha256::new();
    outer.update(outer_key);
    outer.update(inner.finalize());
    outer.finalize().into()
}

fn constant_time_equal(left: &[u8], right: &[u8]) -> bool {
    let mut difference = left.len() ^ right.len();
    for index in 0..left.len().max(right.len()) {
        difference |= usize::from(
            left.get(index).copied().unwrap_or(0) ^ right.get(index).copied().unwrap_or(0),
        );
    }
    difference == 0
}

fn lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn lower_even_hex(value: &str, minimum: usize, maximum: usize) -> bool {
    value.len().is_multiple_of(2)
        && value.len() >= minimum
        && value.len() <= maximum
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(all(test, windows, feature = "codex-provisioning-helper"))]
#[path = "helper/tests.rs"]
mod tests;
