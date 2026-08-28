use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom};
use std::os::windows::ffi::{OsStrExt, OsStringExt};
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_DIRECTORY, FILE_NAME_NORMALIZED, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, GetFileInformationByHandle, GetFileType,
    GetFinalPathNameByHandleW, VOLUME_NAME_DOS,
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
        let canonical =
            std::fs::canonicalize(&spec.artifact.canonical_absolute_path).map_err(|_| {
                identity_mismatch("recorded binding canonical path", expected(spec), None)
            })?;
        if !same_windows_path(&canonical, &spec.artifact.canonical_absolute_path) {
            identity_mismatch(
                "recorded binding canonical path",
                expected(spec),
                path_identity(&canonical),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        let share_mode = match spec.artifact.entrypoint_kind {
            EntryPointKind::NativeExecutable => FILE_SHARE_READ,
            // cmd.exe opens batch files while allowing a writer to coexist.
            // Revalidate the observed file again while cmd.exe is suspended
            // before allowing it to interpret the launcher.
            EntryPointKind::WindowsBatchLauncher => {
                FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
            }
        };
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(share_mode)
            .open(&canonical)
            .map_err(|_| identity_mismatch("recorded binding open", expected(spec), None))?;
        let raw = file.as_raw_handle() as HANDLE;
        // SAFETY: raw is borrowed from a live File.
        if unsafe { GetFileType(raw) } != FILE_TYPE_DISK {
            identity_mismatch(
                "recorded binding file type",
                expected(spec),
                Some(PathIdentity::path_only(canonical)),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        let mut info = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: info is writable and raw remains live for the call.
        if unsafe { GetFileInformationByHandle(raw, &mut info) } == 0
            || info.dwFileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0
        {
            identity_mismatch(
                "recorded binding file metadata",
                expected(spec),
                Some(PathIdentity::path_only(canonical)),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        let bytes = (u64::from(info.nFileSizeHigh) << 32) | u64::from(info.nFileSizeLow);
        if bytes != spec.artifact.byte_len {
            identity_mismatch(
                "recorded binding size",
                expected(spec),
                Some(PathIdentity::with_bytes(canonical, bytes)),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        let final_path = final_path(raw)
            .map_err(|_| identity_mismatch("recorded binding handle path", expected(spec), None))?;
        let observed_final = std::fs::canonicalize(final_path)
            .map_err(|_| identity_mismatch("recorded binding handle path", expected(spec), None))?;
        if !same_windows_path(&observed_final, &canonical) {
            identity_mismatch(
                "recorded binding handle path",
                expected(spec),
                Some(PathIdentity::with_bytes(observed_final, bytes)),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        let digest = hash_file(&mut file, bytes)
            .map_err(|_| identity_mismatch("recorded binding SHA-256", expected(spec), None))?;
        if digest != spec.artifact.sha256 {
            identity_mismatch(
                "recorded binding SHA-256",
                expected(spec),
                Some(PathIdentity::complete(canonical, bytes, digest)),
            );
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
        expected_image: &Path,
    ) -> Result<(), LaunchError> {
        let image_path = process_image_path(process).map_err(|_| {
            identity_mismatch("suspended image", path_identity(expected_image), None)
        })?;
        let observed_image = std::fs::canonicalize(image_path).map_err(|_| {
            identity_mismatch("suspended image", path_identity(expected_image), None)
        })?;
        if !same_windows_path(&observed_image, expected_image) {
            identity_mismatch(
                "suspended image",
                path_identity(expected_image),
                path_identity(&observed_image),
            );
            return Err(LaunchError::IdentityMismatch);
        }
        self.revalidate(spec, "suspended .cmd")
    }

    /// A batch launcher cannot be held exclusively because `cmd.exe` must
    /// reopen it. Recheck the observed launcher immediately before spawn and
    /// again while the host is suspended.
    pub(super) fn revalidate(
        &self,
        spec: &LaunchSpec,
        checkpoint: &'static str,
    ) -> Result<(), LaunchError> {
        let current = Self::open(spec)?;
        (current.identity == self.identity
            && current.bytes == self.bytes
            && current.digest == self.digest)
            .then_some(())
            .ok_or_else(|| {
                identity_mismatch(
                    checkpoint,
                    Some(PathIdentity::complete(
                        self.canonical_path.clone(),
                        self.bytes,
                        self.digest,
                    )),
                    Some(PathIdentity::complete(
                        current.canonical_path.clone(),
                        current.bytes,
                        current.digest,
                    )),
                )
            })
    }

    pub(super) fn sha256(&self) -> [u8; 32] {
        self.digest
    }

    pub(super) fn byte_len(&self) -> u64 {
        self.bytes
    }

    pub(super) fn read_launcher_bytes(&self, maximum: u64) -> Result<Vec<u8>, LaunchError> {
        if self.bytes > maximum {
            return Err(LaunchError::IdentityMismatch);
        }
        let mut file = self.file.try_clone().map_err(|_| LaunchError::Io)?;
        file.seek(SeekFrom::Start(0)).map_err(|_| LaunchError::Io)?;
        let mut bytes = Vec::with_capacity(usize::try_from(self.bytes).unwrap_or(usize::MAX));
        file.take(self.bytes)
            .read_to_end(&mut bytes)
            .map_err(|_| LaunchError::Io)?;
        (bytes.len() as u64 == self.bytes)
            .then_some(bytes)
            .ok_or(LaunchError::IdentityMismatch)
    }
}

#[derive(Clone)]
struct PathIdentity {
    path: PathBuf,
    bytes: Option<u64>,
    digest: Option<[u8; 32]>,
}

impl PathIdentity {
    fn path_only(path: PathBuf) -> Self {
        Self {
            path,
            bytes: None,
            digest: None,
        }
    }

    fn with_bytes(path: PathBuf, bytes: u64) -> Self {
        Self {
            path,
            bytes: Some(bytes),
            digest: None,
        }
    }

    fn complete(path: PathBuf, bytes: u64, digest: [u8; 32]) -> Self {
        Self {
            path,
            bytes: Some(bytes),
            digest: Some(digest),
        }
    }
}

fn expected(spec: &LaunchSpec) -> Option<PathIdentity> {
    Some(PathIdentity::complete(
        spec.artifact.canonical_absolute_path.clone(),
        spec.artifact.byte_len,
        spec.artifact.sha256,
    ))
}

fn path_identity(path: &Path) -> Option<PathIdentity> {
    let canonical = std::fs::canonicalize(path).ok()?;
    let mut file = File::open(&canonical).ok()?;
    let bytes = file.metadata().ok()?.len();
    let digest = hash_file(&mut file, bytes).ok()?;
    Some(PathIdentity::complete(canonical, bytes, digest))
}

fn identity_mismatch(
    checkpoint: &'static str,
    expected: Option<PathIdentity>,
    observed: Option<PathIdentity>,
) -> LaunchError {
    tracing::warn!(
        checkpoint,
        expected_path = ?expected.as_ref().map(|identity| &identity.path),
        expected_bytes = ?expected.as_ref().and_then(|identity| identity.bytes),
        expected_sha256 = ?expected.as_ref().and_then(|identity| identity.digest).map(hex_digest),
        observed_path = ?observed.as_ref().map(|identity| &identity.path),
        observed_bytes = ?observed.as_ref().and_then(|identity| identity.bytes),
        observed_sha256 = ?observed.as_ref().and_then(|identity| identity.digest).map(hex_digest),
        "ACP launch identity mismatch"
    );
    LaunchError::IdentityMismatch
}

fn hex_digest(digest: [u8; 32]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn same_windows_path(left: &Path, right: &Path) -> bool {
    let left = normalized_windows_path(left);
    let right = normalized_windows_path(right);
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(left, right)| ascii_lower(*left) == ascii_lower(right))
}

/// Win32 APIs may return an extended-length (`\\?\`) spelling for the same
/// file that discovery recorded as an ordinary DOS path. Remove only that
/// representation prefix; the complete remaining path still compares exactly
/// (case-insensitively, matching the Windows filesystem).
fn normalized_windows_path(path: &Path) -> Vec<u16> {
    let path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    const VERBATIM_DOS: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const VERBATIM_UNC: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    if starts_with_ascii_ignore_case(&path, VERBATIM_UNC) {
        [
            vec![b'\\' as u16, b'\\' as u16],
            path[VERBATIM_UNC.len()..].to_vec(),
        ]
        .concat()
    } else if starts_with_ascii_ignore_case(&path, VERBATIM_DOS) {
        path[VERBATIM_DOS.len()..].to_vec()
    } else {
        path
    }
}

fn starts_with_ascii_ignore_case(value: &[u16], prefix: &[u16]) -> bool {
    value.len() >= prefix.len()
        && value
            .iter()
            .zip(prefix)
            .all(|(value, prefix)| ascii_lower(*value) == ascii_lower(*prefix))
}

fn ascii_lower(value: u16) -> u16 {
    if (b'A' as u16..=b'Z' as u16).contains(&value) {
        value + (b'a' - b'A') as u16
    } else {
        value
    }
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

pub(super) fn process_image_path(process: HANDLE) -> Result<PathBuf, LaunchError> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_host_paths_ignore_case_without_accepting_a_different_image() {
        assert!(same_windows_path(
            Path::new(r"C:\WINDOWS\System32\cmd.exe"),
            Path::new(r"c:\windows\system32\CMD.EXE"),
        ));
        assert!(same_windows_path(
            Path::new(r"C:\Users\Username\AppData\Roaming\npm\codex.cmd"),
            Path::new(r"\\?\C:\Users\Username\AppData\Roaming\npm\codex.cmd"),
        ));
        assert!(!same_windows_path(
            Path::new(r"C:\WINDOWS\System32\cmd.exe"),
            Path::new(r"C:\WINDOWS\System32\not-cmd.exe"),
        ));
    }
}
