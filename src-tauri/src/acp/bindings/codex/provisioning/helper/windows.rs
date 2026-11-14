use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::Read;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Security::{WinBuiltinAdministratorsSid, WinLocalSystemSid};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
};

use super::transaction::{
    HelperTransactionPlatform, NamespaceObservation, NodeIdentity, execute_boundary_recover,
    execute_boundary_remove, execute_boundary_update, finalize,
};
use super::{AuthenticatedHelperRequest, ElevatedHelperError};
mod native;
mod platform;
mod security;
mod transaction_lock;
use crate::acp::bindings::codex::managed::{
    ManagedStagingPrerequisite, current_process_user_sid, inspect_provisioned_managed_staging,
    validate_provisioned_package_acl, validate_provisioned_state_root,
};
use crate::acp::bindings::codex::physical::{
    ConfiguredManagedPackageRoot, ExpectedPhysicalFile, InspectionClaims,
    PhysicallyInspectedPackage, decode_digest, inspect_claims, inspect_managed_package,
    same_native_path,
};
use crate::acp::bindings::codex::provisioning::{AclPlan, SealedHandoff, hex};
use crate::{
    acp::bindings::codex::manifest::MetadataValidatedPacket,
    acp_protocol::codex_protocol::pinned_codex_version,
};
use native::{
    bounded_names, cleanup_completed_stage, create_initial_journal, handle_path, node_identity,
    open_destination_root, open_mutation_directory, open_rename_directory,
    remove_exact_directory_tree, rename_exact, replace_journal,
};
use security::{Principals, apply_acl, apply_acl_to_exact_node, well_known_sid};
use transaction_lock::TransactionLock;

const RECEIPT_BYTES_MAX: usize = 64 * 1024;
const HELD_PACKAGE_NAME: &str =
    concat!(".sage-codex-", pinned_codex_version!(), "-previous-package");
const HELD_STATE_NAME: &str = concat!(".sage-codex-", pinned_codex_version!(), "-previous-state");

/// Production construction requires the boundary's unforgeable
/// elevated-authority token.
pub(crate) struct ElevatedHelperProcess(());

#[cfg(feature = "codex-provisioning-helper")]
impl ElevatedHelperProcess {
    pub(super) fn from_authenticated_boundary(
        _authority: &crate::codex_helper_boundary::ElevatedBoundaryAuthority,
    ) -> Self {
        Self(())
    }
}

#[cfg(test)]
impl ElevatedHelperProcess {
    fn fixture() -> Self {
        Self(())
    }
}

pub(crate) fn execute_authenticated_request(
    _process: &ElevatedHelperProcess,
    request: &AuthenticatedHelperRequest,
    packet: &MetadataValidatedPacket,
) -> Result<ManagedStagingPrerequisite, ElevatedHelperError> {
    let mut platform = WindowsPlatform::new(request, packet)?;
    finalize(request, &mut platform)
}

pub(crate) fn execute_authenticated_recover(
    _process: &ElevatedHelperProcess,
    request: &AuthenticatedHelperRequest,
    packet: &MetadataValidatedPacket,
) -> Result<ManagedStagingPrerequisite, ElevatedHelperError> {
    let mut platform = WindowsPlatform::new(request, packet)?;
    execute_boundary_recover(request, &mut platform)
}

pub(crate) fn execute_authenticated_update(
    _process: &ElevatedHelperProcess,
    request: &AuthenticatedHelperRequest,
    packet: &MetadataValidatedPacket,
) -> Result<ManagedStagingPrerequisite, ElevatedHelperError> {
    let mut platform = WindowsPlatform::new(request, packet)?;
    execute_boundary_update(request, &mut platform)
}

pub(crate) fn execute_authenticated_remove(
    _process: &ElevatedHelperProcess,
    request: &AuthenticatedHelperRequest,
    packet: &MetadataValidatedPacket,
) -> Result<(), ElevatedHelperError> {
    let mut platform = WindowsPlatform::new(request, packet)?;
    execute_boundary_remove(request, &mut platform)
}

struct WindowsPlatform {
    _transaction_lock: TransactionLock,
    packet: MetadataValidatedPacket,
    stage_root: PathBuf,
    stage_package: PathBuf,
    stage_state: PathBuf,
    package_target: PathBuf,
    state_target: PathBuf,
    receipt: SealedHandoff,
    expected_files: Vec<ExpectedPhysicalFile>,
    aggregate_bytes: u64,
    package_acl: AclPlan,
    state_acl: AclPlan,
    principals: Principals,
    stage_proof: Option<PhysicallyInspectedPackage>,
    stage_root_handle: Option<File>,
    package_parent_handle: File,
    state_parent_handle: File,
    state_handle: Option<File>,
    state_identity: Option<NodeIdentity>,
    held_package: Option<PhysicallyInspectedPackage>,
    held_state: Option<File>,
    held_state_identity: Option<NodeIdentity>,
    creation_nonce: String,
}

impl WindowsPlatform {
    fn new(
        request: &AuthenticatedHelperRequest,
        packet: &MetadataValidatedPacket,
    ) -> Result<Self, ElevatedHelperError> {
        let payload = &request.payload.handoff.payload;
        let user = current_process_user_sid().map_err(|_| ElevatedHelperError::HelperIdentity)?;
        if hex(&user) != payload.current_user_sid_hex {
            return Err(ElevatedHelperError::HelperIdentity);
        }
        let expected_files = payload
            .files
            .iter()
            .map(|file| {
                Ok(ExpectedPhysicalFile {
                    relative_path: file.relative_path.clone(),
                    sha256: decode_digest(&file.sha256)
                        .map_err(|_| ElevatedHelperError::HandoffIdentity)?,
                    byte_len: file.byte_len,
                })
            })
            .collect::<Result<Vec<_>, ElevatedHelperError>>()?;
        let transaction_lock = TransactionLock::acquire(request.package_target())?;
        let stage_root = request.stage_root().to_owned();
        let stage_root_handle = if exact_directory_presence(&stage_root)? {
            Some(open_destination_root(&stage_root)?)
        } else {
            None
        };
        let package_parent = request
            .package_target()
            .parent()
            .ok_or(ElevatedHelperError::HandoffIdentity)?;
        let package_parent_handle = open_destination_root(package_parent)?;
        if node_identity(transaction_lock.parent_handle())?
            != node_identity(&package_parent_handle)?
        {
            return Err(ElevatedHelperError::Drift);
        }
        let state_parent = request
            .state_target()
            .parent()
            .ok_or(ElevatedHelperError::HandoffIdentity)?;
        let state_parent_handle = open_destination_root(state_parent)?;
        Ok(Self {
            _transaction_lock: transaction_lock,
            packet: packet.clone(),
            stage_root,
            stage_package: request.stage_root().join("package"),
            stage_state: request.stage_root().join("state"),
            package_target: request.package_target().to_owned(),
            state_target: request.state_target().to_owned(),
            receipt: request.payload.handoff.clone(),
            expected_files,
            aggregate_bytes: payload.aggregate_byte_len,
            package_acl: payload.package_acl.clone(),
            state_acl: payload.state_acl.clone(),
            principals: Principals {
                user,
                administrators: well_known_sid(WinBuiltinAdministratorsSid)?,
                system: well_known_sid(WinLocalSystemSid)?,
            },
            stage_proof: None,
            stage_root_handle,
            package_parent_handle,
            state_parent_handle,
            state_handle: None,
            state_identity: None,
            held_package: None,
            held_state: None,
            held_state_identity: None,
            creation_nonce: request.creation_nonce().to_owned(),
        })
    }

    fn inspect_at(&self, root: PathBuf) -> Result<PhysicallyInspectedPackage, ElevatedHelperError> {
        inspect_claims(InspectionClaims {
            package_root: root,
            codex_home: self.state_target.clone(),
            files: self.expected_files.clone(),
            aggregate_bytes: self.aggregate_bytes,
        })
        .map_err(|_| ElevatedHelperError::Drift)
    }
}

fn validate_stage_closure(
    stage: &Path,
    state: &Path,
    receipt: &SealedHandoff,
    package_present: bool,
    state_published: bool,
    holding_published_pair: bool,
) -> Result<(), ElevatedHelperError> {
    let mut expected = ["handoff.json"]
        .into_iter()
        .map(Into::into)
        .collect::<BTreeSet<_>>();
    if package_present {
        expected.insert("package".into());
    }
    if !state_published {
        expected.insert("state".into());
    }
    if holding_published_pair {
        expected.insert(HELD_PACKAGE_NAME.into());
        expected.insert(HELD_STATE_NAME.into());
    }
    let names = bounded_names(stage, expected.len(), ElevatedHelperError::Drift)?;
    if names != expected
        || (!state_published && !bounded_names(state, 0, ElevatedHelperError::Drift)?.is_empty())
    {
        return Err(ElevatedHelperError::Drift);
    }
    let bytes = read_bounded_file(&stage.join("handoff.json"), RECEIPT_BYTES_MAX)?
        .ok_or(ElevatedHelperError::Drift)?;
    let observed: SealedHandoff =
        serde_json::from_slice(&bytes).map_err(|_| ElevatedHelperError::Drift)?;
    if &observed != receipt {
        return Err(ElevatedHelperError::Drift);
    }
    Ok(())
}

fn read_bounded_file(path: &Path, limit: usize) -> Result<Option<Vec<u8>>, ElevatedHelperError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ElevatedHelperError::Journal),
    };
    if !metadata.file_type().is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || metadata.len() > limit as u64
    {
        return Err(ElevatedHelperError::Journal);
    }
    let mut file = OpenOptions::new()
        .read(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| ElevatedHelperError::Journal)?;
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    Read::by_ref(&mut file)
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ElevatedHelperError::Journal)?;
    if bytes.len() > limit {
        return Err(ElevatedHelperError::Journal);
    }
    Ok(Some(bytes))
}

fn remove_exact_file(path: &Path) -> Result<(), ElevatedHelperError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ElevatedHelperError::Journal)?;
    if !metadata.file_type().is_file()
        || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    {
        return Err(ElevatedHelperError::Journal);
    }
    fs::remove_file(path).map_err(|_| ElevatedHelperError::Journal)
}

fn exact_directory_presence(path: &Path) -> Result<bool, ElevatedHelperError> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.file_type().is_dir()
                && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 =>
        {
            Ok(true)
        }
        Ok(_) => Err(ElevatedHelperError::Drift),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(_) => Err(ElevatedHelperError::Drift),
    }
}
