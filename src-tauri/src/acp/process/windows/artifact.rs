use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::OsStringExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_NAME_NORMALIZED, FILE_SHARE_READ,
    FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType, GetFinalPathNameByHandleW,
    VOLUME_NAME_DOS,
};
use windows_sys::Win32::System::Threading::QueryFullProcessImageNameW;

use super::super::supervisor::{EntryPointKind, LaunchError, LaunchSpec};
use super::MAX_WINDOWS_WIDE;

#[derive(Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u32,
    index: u64,
}

pub(super) struct VerifiedArtifact {
    pub(super) file: File,
    pub(super) canonical_path: PathBuf,
    identity: FileIdentity,
    bytes: u64,
    digest: [u8; 32],
}

impl VerifiedArtifact {
    pub(super) fn open(spec: &LaunchSpec) -> Result<Self, LaunchError> {
        if spec.artifact.entrypoint_kind != EntryPointKind::NativeExecutable {
            return Err(LaunchError::IdentityMismatch);
        }
        let canonical = std::fs::canonicalize(&spec.artifact.canonical_absolute_path)
            .map_err(|_| LaunchError::IdentityMismatch)?;
        if canonical != spec.artifact.canonical_absolute_path {
            return Err(LaunchError::IdentityMismatch);
        }
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .open(&canonical)
            .map_err(|_| LaunchError::IdentityMismatch)?;
        let raw = file.as_raw_handle() as HANDLE;
        // SAFETY: raw is borrowed from a live File.
        if unsafe { GetFileType(raw) } != FILE_TYPE_DISK {
            return Err(LaunchError::IdentityMismatch);
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: info is writable and raw remains live for the call.
        if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        {
            return Err(LaunchError::IdentityMismatch);
        }
        let bytes = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
        if bytes != spec.artifact.byte_len {
            return Err(LaunchError::IdentityMismatch);
        }
        let final_path = final_path(raw)?;
        if std::fs::canonicalize(final_path).map_err(|_| LaunchError::IdentityMismatch)?
            != canonical
        {
            return Err(LaunchError::IdentityMismatch);
        }
        let digest = hash_file(&mut file, bytes)?;
        if digest != spec.artifact.sha256 {
            return Err(LaunchError::IdentityMismatch);
        }
        Ok(Self {
            file,
            canonical_path: canonical,
            identity: FileIdentity {
                volume: info.dwVolumeSerialNumber,
                index: (u64::from(info.nFileIndexHigh) << 32) | u64::from(info.nFileIndexLow),
            },
            bytes,
            digest,
        })
    }

    pub(super) fn matches_running_image(
        &self,
        process: HANDLE,
        spec: &LaunchSpec,
    ) -> Result<(), LaunchError> {
        let image_path = process_image_path(process)?;
        if std::fs::canonicalize(image_path).map_err(|_| LaunchError::IdentityMismatch)?
            != self.canonical_path
        {
            return Err(LaunchError::IdentityMismatch);
        }
        let running = Self::open(spec)?;
        if running.identity != self.identity
            || running.bytes != self.bytes
            || running.digest != self.digest
        {
            return Err(LaunchError::IdentityMismatch);
        }
        Ok(())
    }

    pub(super) fn sha256(&self) -> [u8; 32] {
        self.digest
    }

    pub(super) fn byte_len(&self) -> u64 {
        self.bytes
    }
}

pub(super) fn validate_directory(path: &Path) -> Result<(), LaunchError> {
    let canonical = std::fs::canonicalize(path).map_err(|_| LaunchError::InvalidAbsolutePath)?;
    if canonical != path || !canonical.is_dir() {
        return Err(LaunchError::InvalidAbsolutePath);
    }
    Ok(())
}

fn hash_file(file: &mut File, bytes: u64) -> Result<[u8; 32], LaunchError> {
    file.seek(SeekFrom::Start(0)).map_err(|_| LaunchError::Io)?;
    let mut reader = file.take(bytes.saturating_add(1));
    let mut hasher = Sha256::new();
    let mut observed = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).map_err(|_| LaunchError::Io)?;
        if read == 0 {
            break;
        }
        observed = observed.saturating_add(read as u64);
        hasher.update(&buffer[..read]);
    }
    if observed != bytes {
        return Err(LaunchError::IdentityMismatch);
    }
    Ok(hasher.finalize().into())
}

fn final_path(handle: HANDLE) -> Result<PathBuf, LaunchError> {
    let mut buffer = vec![0_u16; MAX_WINDOWS_WIDE];
    // SAFETY: buffer is writable and handle is a live disk file.
    let length = unsafe {
        GetFinalPathNameByHandleW(
            handle,
            buffer.as_mut_ptr(),
            u32::try_from(buffer.len()).map_err(|_| LaunchError::IdentityMismatch)?,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        )
    };
    let length = usize::try_from(length).map_err(|_| LaunchError::IdentityMismatch)?;
    if length == 0 || length >= buffer.len() {
        return Err(LaunchError::IdentityMismatch);
    }
    Ok(PathBuf::from(OsString::from_wide(&buffer[..length])))
}

fn process_image_path(process: HANDLE) -> Result<PathBuf, LaunchError> {
    let mut buffer = vec![0_u16; MAX_WINDOWS_WIDE];
    let mut length = u32::try_from(buffer.len()).map_err(|_| LaunchError::IdentityMismatch)?;
    // SAFETY: process has query access and buffer/length are writable.
    if unsafe { QueryFullProcessImageNameW(process, 0, buffer.as_mut_ptr(), &mut length) } == 0 {
        return Err(LaunchError::IdentityMismatch);
    }
    Ok(PathBuf::from(OsString::from_wide(
        &buffer[..length as usize],
    )))
}
