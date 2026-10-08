use std::fs::{self, File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::windows::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Storage::FileSystem::{FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ};

use super::{
    AclPlan, HandoffFile, HandoffPayload, ProvisioningError, RECEIPT_BYTES_MAX, SealedHandoff,
    SourceFile, StagingOperation, VerifiedCodexSource, hex,
};
use crate::acp::bindings::codex::{
    managed::current_process_user_sid,
    physical::{
        InspectionClaims, PhysicallyInspectedPackage, inspect_claims, open_exact_state_root,
    },
};

mod targets;

use targets::{require_absent, require_present, target_exists, validate_root_closure};

#[cfg(test)]
mod test_failpoint;

const STAGE_NAME: &str = ".sage-codex-0.146.0-stage";
const HANDOFF_NAME: &str = "handoff.json";
const PACKAGE_NAME: &str = "package";
const STATE_NAME: &str = "state";
const COPY_BUFFER_BYTES: usize = 64 * 1024;
const COPY_TIME_MAX: Duration = Duration::from_secs(15 * 60);
const FILE_ALL_ACCESS_MASK: u32 = 0x001f_01ff;
const FILE_READ_EXECUTE_MASK: u32 = 0x0012_00a9;
const INHERIT_FILES_AND_DIRECTORIES: u8 = 0x03;

#[derive(Debug)]
pub(super) struct WindowsPreparedStaging {
    stage_root: PathBuf,
    package_root: PathBuf,
    state_root: PathBuf,
    package_target: PathBuf,
    state_target: PathBuf,
    files: Vec<SourceFile>,
    aggregate_bytes: u64,
    receipt: SealedHandoff,
    package_proof: Option<PhysicallyInspectedPackage>,
    receipt_handle: Option<File>,
    state_handle: Option<File>,
    stage_handle: Option<File>,
    operation: StagingOperation,
}

impl WindowsPreparedStaging {
    pub(super) fn revalidate(&self) -> Result<(), ProvisioningError> {
        match self.operation {
            StagingOperation::Publish => {
                require_absent(&self.package_target)?;
                require_absent(&self.state_target)?;
            }
            StagingOperation::Update => {
                require_present(&self.package_target)?;
                require_present(&self.state_target)?;
            }
        }
        validate_root_closure(&self.stage_root)?;
        if fs::read_dir(&self.state_root)
            .map_err(|_| ProvisioningError::StageDrift)?
            .next()
            .is_some()
        {
            return Err(ProvisioningError::StageDrift);
        }
        let observed = read_receipt(&self.stage_root.join(HANDOFF_NAME))?;
        if observed != self.receipt {
            return Err(ProvisioningError::Receipt);
        }
        inspect_claims(InspectionClaims {
            package_root: self.package_root.clone(),
            codex_home: self.state_root.clone(),
            files: expected_files(&self.files),
            aggregate_bytes: self.aggregate_bytes,
        })
        .map_err(|_| ProvisioningError::StageDrift)?;
        Ok(())
    }

    pub(super) fn stage_root(&self) -> &Path {
        &self.stage_root
    }

    #[cfg(test)]
    pub(super) fn corrupt_receipt_for_test(&mut self) {
        let receipt = self
            .receipt_handle
            .as_mut()
            .expect("prepared receipt handle");
        receipt.seek(SeekFrom::Start(0)).unwrap();
        receipt.write_all(b"{}").unwrap();
        receipt.set_len(2).unwrap();
        receipt.sync_all().unwrap();
    }

    fn close(&mut self) {
        self.package_proof.take();
        self.receipt_handle.take();
        self.state_handle.take();
        self.stage_handle.take();
    }
}

impl Drop for WindowsPreparedStaging {
    fn drop(&mut self) {
        self.close();
        let _ = fs::remove_dir_all(&self.stage_root);
    }
}

pub(super) fn prepare(
    source: &VerifiedCodexSource,
) -> Result<WindowsPreparedStaging, ProvisioningError> {
    prepare_for_operation(source, StagingOperation::Publish)
}

pub(super) fn target_operation(
    source: &VerifiedCodexSource,
) -> Result<StagingOperation, ProvisioningError> {
    let package_exists = target_exists(&source.package_target)?;
    let state_exists = target_exists(&source.state_target)?;
    match (package_exists, state_exists) {
        (false, false) => Ok(StagingOperation::Publish),
        (true, true) => Ok(StagingOperation::Update),
        _ => Err(ProvisioningError::StageDrift),
    }
}

pub(super) fn prepare_for_operation(
    source: &VerifiedCodexSource,
    operation: StagingOperation,
) -> Result<WindowsPreparedStaging, ProvisioningError> {
    match operation {
        StagingOperation::Publish => {
            require_absent(&source.package_target)?;
            require_absent(&source.state_target)?;
        }
        StagingOperation::Update => {
            require_present(&source.package_target)?;
            require_present(&source.state_target)?;
        }
    }
    let parent = source
        .package_target
        .parent()
        .ok_or(ProvisioningError::SourcePath)?;
    let stage_root = parent.join(STAGE_NAME);
    match fs::create_dir(&stage_root) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            return Err(ProvisioningError::StageExists);
        }
        Err(_) => return Err(ProvisioningError::Io),
    }
    match prepare_created(source, stage_root.clone(), operation) {
        Ok(prepared) => Ok(prepared),
        Err(error) => match fs::remove_dir_all(&stage_root) {
            Ok(()) => Err(error),
            Err(cleanup) if cleanup.kind() == std::io::ErrorKind::NotFound => Err(error),
            Err(_) => Err(ProvisioningError::Rollback),
        },
    }
}

fn prepare_created(
    source: &VerifiedCodexSource,
    stage_root: PathBuf,
    operation: StagingOperation,
) -> Result<WindowsPreparedStaging, ProvisioningError> {
    let (_, stage_handle) =
        open_exact_state_root(&stage_root).map_err(|_| ProvisioningError::StageDrift)?;
    let package_root = stage_root.join(PACKAGE_NAME);
    let state_root = stage_root.join(STATE_NAME);
    fs::create_dir(&package_root).map_err(|_| ProvisioningError::Io)?;
    fs::create_dir(&state_root).map_err(|_| ProvisioningError::Io)?;
    let (_, package_handle) =
        open_exact_state_root(&package_root).map_err(|_| ProvisioningError::StageDrift)?;
    let (_, state_handle) =
        open_exact_state_root(&state_root).map_err(|_| ProvisioningError::StageDrift)?;

    let started = Instant::now();
    let directories = expected_directories(&source.files)?;
    let mut copy_directory_handles = vec![package_handle];
    for relative in &directories {
        let path = package_root.join(relative);
        fs::create_dir(&path).map_err(|_| ProvisioningError::Io)?;
        let (_, handle) =
            open_exact_state_root(&path).map_err(|_| ProvisioningError::StageDrift)?;
        copy_directory_handles.push(handle);
    }
    for file in &source.files {
        copy_one(source, file, &package_root, started)?;
    }
    source.revalidate()?;
    let package_proof = source.inspect_root(package_root.clone(), state_root.clone())?;

    let receipt = build_receipt(source)?;
    let receipt_path = stage_root.join(HANDOFF_NAME);
    let mut receipt_handle = create_new_file(&receipt_path)?;
    let receipt_bytes = serde_json::to_vec(&receipt).map_err(|_| ProvisioningError::Receipt)?;
    if receipt_bytes.len() > RECEIPT_BYTES_MAX {
        return Err(ProvisioningError::Bound);
    }
    receipt_handle
        .write_all(&receipt_bytes)
        .map_err(|_| ProvisioningError::Io)?;
    receipt_handle
        .sync_all()
        .map_err(|_| ProvisioningError::Sync)?;
    for directory in package_proof
        .held_nodes()
        .iter()
        .filter(|node| node.directory)
    {
        sync_directory(&directory.handle)?;
    }
    sync_directory(&state_handle)?;
    sync_directory(&stage_handle)?;
    drop(copy_directory_handles);

    let prepared = WindowsPreparedStaging {
        stage_root,
        package_root,
        state_root,
        package_target: source.package_target.clone(),
        state_target: source.state_target.clone(),
        files: source.files.clone(),
        aggregate_bytes: source.inspection.byte_len(),
        receipt,
        package_proof: Some(package_proof),
        receipt_handle: Some(receipt_handle),
        state_handle: Some(state_handle),
        stage_handle: Some(stage_handle),
        operation,
    };
    prepared.revalidate()?;
    Ok(prepared)
}

fn copy_one(
    source: &VerifiedCodexSource,
    expected: &SourceFile,
    package_root: &Path,
    started: Instant,
) -> Result<(), ProvisioningError> {
    #[cfg(test)]
    if test_failpoint::run_copy_failpoint() {
        return Err(ProvisioningError::Io);
    }
    if started.elapsed() > COPY_TIME_MAX {
        return Err(ProvisioningError::Bound);
    }
    let source_path = source.source_root().join(&expected.relative_path);
    let node = source
        .inspection
        .held_nodes()
        .iter()
        .find(|node| !node.directory && node.path == source_path)
        .ok_or(ProvisioningError::SourceBinding)?;
    let mut input = node.handle.try_clone().map_err(|_| ProvisioningError::Io)?;
    input
        .seek(SeekFrom::Start(0))
        .map_err(|_| ProvisioningError::Io)?;
    let destination_path = package_root.join(&expected.relative_path);
    let mut output = create_new_file(&destination_path)?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = [0_u8; COPY_BUFFER_BYTES];
    loop {
        if started.elapsed() > COPY_TIME_MAX {
            return Err(ProvisioningError::Bound);
        }
        let read = input.read(&mut buffer).map_err(|_| ProvisioningError::Io)?;
        if read == 0 {
            break;
        }
        total = total
            .checked_add(read as u64)
            .filter(|bytes| *bytes <= expected.byte_len)
            .ok_or(ProvisioningError::CopyIdentity)?;
        output
            .write_all(&buffer[..read])
            .map_err(|_| ProvisioningError::Io)?;
        hasher.update(&buffer[..read]);
    }
    if total != expected.byte_len || <[u8; 32]>::from(hasher.finalize()) != expected.sha256 {
        return Err(ProvisioningError::CopyIdentity);
    }
    output.flush().map_err(|_| ProvisioningError::Io)?;
    output.sync_all().map_err(|_| ProvisioningError::Sync)
}

fn create_new_file(path: &Path) -> Result<File, ProvisioningError> {
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .create_new(true)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    options.open(path).map_err(|_| ProvisioningError::Io)
}

fn expected_directories(files: &[SourceFile]) -> Result<Vec<PathBuf>, ProvisioningError> {
    let mut directories = std::collections::BTreeSet::new();
    for file in files {
        if file.relative_path.components().count() > 8 {
            return Err(ProvisioningError::Bound);
        }
        let mut parent = file.relative_path.parent();
        while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
            directories.insert(path.to_owned());
            parent = path.parent();
        }
    }
    Ok(directories.into_iter().collect())
}

fn build_receipt(source: &VerifiedCodexSource) -> Result<SealedHandoff, ProvisioningError> {
    let sid = current_process_user_sid().map_err(|_| ProvisioningError::Receipt)?;
    let payload = HandoffPayload {
        schema_version: 1,
        binding_id: "codex-app-server".into(),
        runtime_version: "0.146.0".into(),
        creation_nonce: uuid::Uuid::new_v4().simple().to_string(),
        source_proof_sha256: source.source_proof_sha256.clone(),
        aggregate_byte_len: source.inspection.byte_len(),
        files: source
            .files
            .iter()
            .map(|file| HandoffFile {
                relative_path: file.relative_path.clone(),
                sha256: file.sha256_hex.clone(),
                byte_len: file.byte_len,
            })
            .collect(),
        package_target: source.package_target.clone(),
        state_target: source.state_target.clone(),
        current_user_sid_hex: hex(&sid),
        package_acl: AclPlan {
            owner: "builtinAdministrators".into(),
            protected_dacl: true,
            system_access_mask: FILE_ALL_ACCESS_MASK,
            administrators_access_mask: FILE_ALL_ACCESS_MASK,
            current_user_access_mask: FILE_READ_EXECUTE_MASK,
            inheritance_flags: INHERIT_FILES_AND_DIRECTORIES,
        },
        state_acl: AclPlan {
            owner: "currentUser".into(),
            protected_dacl: true,
            system_access_mask: FILE_ALL_ACCESS_MASK,
            administrators_access_mask: FILE_ALL_ACCESS_MASK,
            current_user_access_mask: FILE_ALL_ACCESS_MASK,
            inheritance_flags: INHERIT_FILES_AND_DIRECTORIES,
        },
        acl_application_status: "requiredPrivilegedHelperNotApplied".into(),
        publish_status: "notPublished".into(),
    };
    let bytes = serde_json::to_vec(&payload).map_err(|_| ProvisioningError::Receipt)?;
    if bytes.len() > RECEIPT_BYTES_MAX {
        return Err(ProvisioningError::Bound);
    }
    Ok(SealedHandoff {
        payload,
        payload_sha256: hex(&Sha256::digest(bytes)),
    })
}

fn read_receipt(path: &Path) -> Result<SealedHandoff, ProvisioningError> {
    let metadata = fs::symlink_metadata(path).map_err(|_| ProvisioningError::Receipt)?;
    if !metadata.file_type().is_file() || metadata.len() as usize > RECEIPT_BYTES_MAX {
        return Err(ProvisioningError::Receipt);
    }
    let bytes = fs::read(path).map_err(|_| ProvisioningError::Receipt)?;
    let receipt: SealedHandoff =
        serde_json::from_slice(&bytes).map_err(|_| ProvisioningError::Receipt)?;
    let payload = serde_json::to_vec(&receipt.payload).map_err(|_| ProvisioningError::Receipt)?;
    if receipt.payload_sha256 != hex(&Sha256::digest(payload)) {
        return Err(ProvisioningError::Receipt);
    }
    Ok(receipt)
}

fn expected_files(
    files: &[SourceFile],
) -> Vec<crate::acp::bindings::codex::physical::ExpectedPhysicalFile> {
    files
        .iter()
        .map(
            |file| crate::acp::bindings::codex::physical::ExpectedPhysicalFile {
                relative_path: file.relative_path.clone(),
                sha256: file.sha256,
                byte_len: file.byte_len,
            },
        )
        .collect()
}

fn sync_directory(directory: &File) -> Result<(), ProvisioningError> {
    match directory.sync_all() {
        Ok(()) => Ok(()),
        // Windows does not support FlushFileBuffers for ordinary directory
        // handles. File contents and the receipt are still flushed above; the
        // directory flush is attempted and skipped only for that documented
        // platform limitation.
        Err(error)
            if error.kind() == std::io::ErrorKind::PermissionDenied
                || matches!(error.raw_os_error(), Some(5 | 6)) =>
        {
            Ok(())
        }
        Err(_) => Err(ProvisioningError::Sync),
    }
}

#[cfg(test)]
pub(super) fn inject_copy_failure_after(successful_copies: usize) {
    test_failpoint::inject_copy_failure_after(successful_copies);
}
