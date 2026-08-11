use std::collections::BTreeSet;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use windows_sys::Win32::Security::{WinBuiltinAdministratorsSid, WinLocalSystemSid};
use windows_sys::Win32::Storage::FileSystem::{
    FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT, MOVEFILE_REPLACE_EXISTING,
    MOVEFILE_WRITE_THROUGH, MoveFileExW,
};

use super::transaction::{HelperTransactionPlatform, NamespaceObservation, finalize};
use super::{AuthenticatedHelperRequest, ElevatedHelperError};
#[path = "windows/security.rs"]
mod security;
#[path = "windows/transaction_lock.rs"]
mod transaction_lock;
use crate::acp::bindings::codex::managed::{
    ManagedStagingPrerequisite, current_process_user_sid, inspect_provisioned_managed_staging,
    validate_provisioned_package_acl, validate_provisioned_state_root,
};
use crate::acp::bindings::codex::manifest::MetadataValidatedPacket;
use crate::acp::bindings::codex::physical::{
    ConfiguredManagedPackageRoot, ExpectedPhysicalFile, InspectionClaims,
    PhysicallyInspectedPackage, decode_digest, inspect_claims, inspect_managed_package,
};
use crate::acp::bindings::codex::provisioning::{AclPlan, SealedHandoff, hex};
use security::{Principals, apply_acl, apply_acl_to_exact_node, open_acl_node, well_known_sid};
use transaction_lock::TransactionLock;

const RECEIPT_BYTES_MAX: usize = 64 * 1024;
const NEXT_SUFFIX: &str = ".next";

/// Capability reserved for the separately built and verified elevated helper
/// process. There is intentionally no production constructor in this lane.
pub(crate) struct ElevatedHelperProcess(());

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
        Ok(Self {
            _transaction_lock: TransactionLock::acquire(request.package_target())?,
            packet: packet.clone(),
            stage_root: request.stage_root().to_owned(),
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

impl HelperTransactionPlatform for WindowsPlatform {
    type Proof = ManagedStagingPrerequisite;

    fn read_journal(
        &mut self,
        path: &Path,
        byte_limit: usize,
    ) -> Result<Option<Vec<u8>>, ElevatedHelperError> {
        read_bounded_file(path, byte_limit)
    }

    fn create_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        write_new_synced(path, bytes)
    }

    fn replace_journal(&mut self, path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
        let mut next_name = path.as_os_str().to_os_string();
        next_name.push(NEXT_SUFFIX);
        let next = PathBuf::from(next_name);
        remove_exact_file_if_present(&next)?;
        write_new_synced(&next, bytes)?;
        rename_write_through(&next, path, true).inspect_err(|_| {
            let _ = fs::remove_file(&next);
        })
    }

    fn remove_journal(&mut self, path: &Path) -> Result<(), ElevatedHelperError> {
        remove_exact_file(path)
    }

    fn observe(&mut self) -> Result<NamespaceObservation, ElevatedHelperError> {
        Ok(NamespaceObservation {
            stage_package: exact_directory_presence(&self.stage_package)?,
            final_package: exact_directory_presence(&self.package_target)?,
            final_state: exact_directory_presence(&self.state_target)?,
        })
    }

    fn revalidate_stage(&mut self) -> Result<(), ElevatedHelperError> {
        validate_stage_closure(
            &self.stage_root,
            &self.stage_state,
            &self.receipt,
            self.stage_package.exists(),
        )?;
        self.stage_proof = Some(self.inspect_at(self.stage_package.clone())?);
        Ok(())
    }

    fn revalidate_stage_acl(&mut self) -> Result<(), ElevatedHelperError> {
        validate_provisioned_package_acl(
            self.stage_proof
                .as_ref()
                .ok_or(ElevatedHelperError::Drift)?,
        )
        .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn revalidate_state(&mut self) -> Result<(), ElevatedHelperError> {
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn apply_package_acl(&mut self) -> Result<(), ElevatedHelperError> {
        let proof = self
            .stage_proof
            .as_ref()
            .ok_or(ElevatedHelperError::Drift)?;
        for node in proof.held_nodes() {
            apply_acl_to_exact_node(
                node,
                &self.package_acl,
                &self.principals.administrators,
                &self.principals,
            )?;
        }
        validate_provisioned_package_acl(proof).map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn create_state_with_acl(&mut self) -> Result<(), ElevatedHelperError> {
        fs::create_dir(&self.state_target).map_err(|error| {
            if error.kind() == std::io::ErrorKind::AlreadyExists {
                ElevatedHelperError::Collision
            } else {
                ElevatedHelperError::Publication
            }
        })?;
        let node = open_acl_node(&self.state_target, true)?;
        apply_acl(
            &node,
            &self.state_acl,
            &self.principals.user,
            &self.principals,
            true,
        )?;
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::AclApplication)
    }

    fn publish_package_write_through(&mut self) -> Result<(), ElevatedHelperError> {
        self.revalidate_stage_acl()?;
        self.stage_proof.take();
        rename_write_through(&self.stage_package, &self.package_target, false)
    }

    fn final_managed_proof(&mut self) -> Result<Self::Proof, ElevatedHelperError> {
        inspect_provisioned_managed_staging(&self.packet)
            .map_err(|_| ElevatedHelperError::FinalProof)
    }

    fn remove_completed_stage(&mut self) -> Result<(), ElevatedHelperError> {
        if exact_directory_presence(&self.stage_root)? {
            let names = fs::read_dir(&self.stage_root)
                .map_err(|_| ElevatedHelperError::Cleanup)?
                .map(|entry| entry.map(|entry| entry.file_name()))
                .collect::<Result<BTreeSet<_>, _>>()
                .map_err(|_| ElevatedHelperError::Cleanup)?;
            let allowed = ["handoff.json", "state"]
                .into_iter()
                .map(Into::into)
                .collect::<BTreeSet<_>>();
            if !names.is_subset(&allowed) {
                return Err(ElevatedHelperError::Cleanup);
            }
        }
        remove_exact_file_if_present(&self.stage_root.join("handoff.json"))?;
        if exact_directory_presence(&self.stage_state)? {
            if fs::read_dir(&self.stage_state)
                .map_err(|_| ElevatedHelperError::Cleanup)?
                .next()
                .is_some()
            {
                return Err(ElevatedHelperError::Cleanup);
            }
            fs::remove_dir(&self.stage_state).map_err(|_| ElevatedHelperError::Cleanup)?;
        }
        if exact_directory_presence(&self.stage_root)? {
            fs::remove_dir(&self.stage_root).map_err(|_| ElevatedHelperError::Cleanup)?;
        }
        Ok(())
    }

    fn rollback_package(&mut self) -> Result<(), ElevatedHelperError> {
        let root = ConfiguredManagedPackageRoot::new(self.package_target.clone())
            .map_err(|_| ElevatedHelperError::Rollback)?;
        let proof = inspect_managed_package(&self.packet, &root)
            .map_err(|_| ElevatedHelperError::Rollback)?;
        validate_provisioned_package_acl(&proof).map_err(|_| ElevatedHelperError::Rollback)?;
        drop(proof);
        rename_write_through(&self.package_target, &self.stage_package, false)
            .map_err(|_| ElevatedHelperError::Rollback)
    }

    fn rollback_state(&mut self) -> Result<(), ElevatedHelperError> {
        validate_provisioned_state_root(&self.state_target)
            .map_err(|_| ElevatedHelperError::Rollback)?;
        if fs::read_dir(&self.state_target)
            .map_err(|_| ElevatedHelperError::Rollback)?
            .next()
            .is_some()
        {
            return Err(ElevatedHelperError::Rollback);
        }
        fs::remove_dir(&self.state_target).map_err(|_| ElevatedHelperError::Rollback)
    }
}

fn validate_stage_closure(
    stage: &Path,
    state: &Path,
    receipt: &SealedHandoff,
    package_present: bool,
) -> Result<(), ElevatedHelperError> {
    let names = fs::read_dir(stage)
        .map_err(|_| ElevatedHelperError::Drift)?
        .map(|entry| entry.map(|entry| entry.file_name()))
        .collect::<Result<BTreeSet<_>, _>>()
        .map_err(|_| ElevatedHelperError::Drift)?;
    let mut expected = ["handoff.json", "state"]
        .into_iter()
        .map(Into::into)
        .collect::<BTreeSet<_>>();
    if package_present {
        expected.insert("package".into());
    }
    if names != expected
        || fs::read_dir(state)
            .map_err(|_| ElevatedHelperError::Drift)?
            .next()
            .is_some()
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

fn write_new_synced(path: &Path, bytes: &[u8]) -> Result<(), ElevatedHelperError> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .share_mode(0)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
        .map_err(|_| ElevatedHelperError::Journal)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| ElevatedHelperError::Journal)
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

fn remove_exact_file_if_present(path: &Path) -> Result<(), ElevatedHelperError> {
    match fs::symlink_metadata(path) {
        Ok(_) => remove_exact_file(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(ElevatedHelperError::Cleanup),
    }
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

fn rename_write_through(from: &Path, to: &Path, replace: bool) -> Result<(), ElevatedHelperError> {
    if !replace && fs::symlink_metadata(to).is_ok() {
        return Err(ElevatedHelperError::Collision);
    }
    let mut from = from.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut to = to.as_os_str().encode_wide().collect::<Vec<_>>();
    from.push(0);
    to.push(0);
    let flags = MOVEFILE_WRITE_THROUGH
        | if replace {
            MOVEFILE_REPLACE_EXISTING
        } else {
            0
        };
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), flags) } == 0 {
        return Err(ElevatedHelperError::Publication);
    }
    Ok(())
}
