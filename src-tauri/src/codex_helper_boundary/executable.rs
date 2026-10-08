#[cfg(windows)]
use std::fs::File;
#[cfg(windows)]
use std::fs::OpenOptions;
#[cfg(windows)]
use std::io::{Read, Seek, SeekFrom};
#[cfg(windows)]
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
#[cfg(windows)]
use std::os::windows::io::AsRawHandle;
use std::path::Path;
#[cfg(windows)]
use std::path::PathBuf;

#[cfg(windows)]
use sha2::{Digest, Sha256};
#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    BY_HANDLE_FILE_INFORMATION, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_SHARE_READ, GetFileInformationByHandle,
};

use super::protocol::BoundaryError;
#[cfg(windows)]
use super::protocol::{HELPER_EXECUTABLE_BYTES_MAX, HelperIdentity};

#[cfg(windows)]
pub(crate) struct VerifiedHelperExecutable {
    path: PathBuf,
    identity: HelperIdentity,
    _file: File,
}

#[cfg(windows)]
impl VerifiedHelperExecutable {
    pub(crate) fn open(path: &Path) -> Result<Self, BoundaryError> {
        if !path.is_absolute() {
            return Err(BoundaryError::HelperIdentity);
        }
        let canonical = path
            .canonicalize()
            .map_err(|_| BoundaryError::HelperIdentity)?;
        let mut file = OpenOptions::new()
            .read(true)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(&canonical)
            .map_err(|_| BoundaryError::HelperIdentity)?;
        let before = file.metadata().map_err(|_| BoundaryError::HelperIdentity)?;
        let before_identity = handle_identity(&file)?;
        if !before.file_type().is_file()
            || before.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
            || before_identity.nNumberOfLinks != 1
            || before.file_size() == 0
            || before.file_size() > HELPER_EXECUTABLE_BYTES_MAX
        {
            return Err(BoundaryError::HelperIdentity);
        }
        file.seek(SeekFrom::Start(0))
            .map_err(|_| BoundaryError::HelperIdentity)?;
        let mut hasher = Sha256::new();
        let mut copied = 0_u64;
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let read = file
                .read(&mut buffer)
                .map_err(|_| BoundaryError::HelperIdentity)?;
            if read == 0 {
                break;
            }
            copied = copied
                .checked_add(read as u64)
                .filter(|bytes| *bytes <= HELPER_EXECUTABLE_BYTES_MAX)
                .ok_or(BoundaryError::HelperIdentity)?;
            hasher.update(&buffer[..read]);
        }
        let after = file.metadata().map_err(|_| BoundaryError::HelperIdentity)?;
        let after_identity = handle_identity(&file)?;
        if copied != before.file_size()
            || before_identity.dwVolumeSerialNumber != after_identity.dwVolumeSerialNumber
            || before_identity.nFileIndexHigh != after_identity.nFileIndexHigh
            || before_identity.nFileIndexLow != after_identity.nFileIndexLow
            || before.last_write_time() != after.last_write_time()
            || before.file_size() != after.file_size()
        {
            return Err(BoundaryError::HelperIdentity);
        }
        Ok(Self {
            path: canonical,
            identity: HelperIdentity {
                sha256: hasher.finalize().into(),
                byte_len: copied,
            },
            _file: file,
        })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn identity(&self) -> HelperIdentity {
        self.identity
    }
}

#[cfg(not(windows))]
pub(crate) fn open(_path: &Path) -> Result<(), BoundaryError> {
    Err(BoundaryError::HelperIdentity)
}

#[cfg(windows)]
fn handle_identity(file: &File) -> Result<BY_HANDLE_FILE_INFORMATION, BoundaryError> {
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    if unsafe { GetFileInformationByHandle(file.as_raw_handle().cast(), &mut information) } == 0 {
        return Err(BoundaryError::HelperIdentity);
    }
    Ok(information)
}

#[cfg(test)]
mod tests {
    #[cfg(windows)]
    use std::fs::{self, OpenOptions};

    #[cfg(windows)]
    use sha2::{Digest, Sha256};
    #[cfg(windows)]
    use uuid::Uuid;

    #[cfg(windows)]
    use super::*;

    #[cfg(windows)]
    struct TempRoot(PathBuf);

    #[cfg(windows)]
    impl TempRoot {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!("sage-helper-image-{}", Uuid::new_v4()));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    #[cfg(windows)]
    impl Drop for TempRoot {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    #[cfg(windows)]
    #[test]
    fn exact_image_is_hashed_and_held_against_write_or_replace() {
        let root = TempRoot::new();
        let path = root.0.join("sage-codex-0146-elevated-helper.exe");
        fs::write(&path, b"exact-helper-image").unwrap();

        let verified = VerifiedHelperExecutable::open(&path).unwrap();

        assert_eq!(verified.path(), path.canonicalize().unwrap());
        assert_eq!(verified.identity().byte_len, 18);
        assert_eq!(
            verified.identity().sha256,
            <[u8; 32]>::from(Sha256::digest(b"exact-helper-image"))
        );
        assert!(OpenOptions::new().write(true).open(&path).is_err());
        assert!(fs::rename(&path, root.0.join("replaced.exe")).is_err());
        drop(verified);
        fs::rename(&path, root.0.join("replaced.exe")).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn hardlinked_and_empty_images_are_rejected() {
        let root = TempRoot::new();
        let empty = root.0.join("empty.exe");
        fs::write(&empty, b"").unwrap();
        assert!(matches!(
            VerifiedHelperExecutable::open(&empty),
            Err(BoundaryError::HelperIdentity)
        ));

        let linked = root.0.join("linked.exe");
        fs::write(&linked, b"helper").unwrap();
        fs::hard_link(&linked, root.0.join("alias.exe")).unwrap();
        assert!(matches!(
            VerifiedHelperExecutable::open(&linked),
            Err(BoundaryError::HelperIdentity)
        ));
    }

    #[cfg(not(windows))]
    #[test]
    fn helper_image_open_fails_closed_outside_windows() {
        use crate::codex_helper_boundary::protocol::BoundaryError;

        assert!(matches!(
            super::open(std::path::Path::new("/tmp/sage-helper-image.exe")),
            Err(BoundaryError::HelperIdentity)
        ));
    }
}
