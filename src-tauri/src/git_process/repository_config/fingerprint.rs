//! Paths are resolved by the operating system exactly as Git resolves them:
//! symlinks, junctions and mount points are followed, and other reparse
//! points (cloud placeholders, say) are ordinary files and directories.
//! Nothing is refused for being a link. Instead every directory is stamped
//! with the identity of the object the path reaches (volume serial and file
//! ID on Windows, device and inode elsewhere), so retargeting a link changes
//! the stamp and invalidates the verdict.

use std::{
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

use sha2::{Digest, Sha256};

/// Larger watched files are refused rather than read on every call.
const MAX_WATCHED_FILE_BYTES: u64 = 256 * 1024;

pub(super) fn watch(path: PathBuf) -> (PathBuf, Fingerprint) {
    let fingerprint = Fingerprint::of(&path);
    (path, fingerprint)
}

pub(super) fn watch_reading(path: PathBuf) -> ((PathBuf, Fingerprint), Option<Vec<u8>>) {
    let (fingerprint, bytes) = Fingerprint::read(&path);
    ((path, fingerprint), bytes)
}

pub(super) type ObjectId = (u64, u64);

/// Regular files are compared by their length and SHA-256 digest, which no
/// timestamp or size trick can forge, so a verdict keeps 40 bytes per file
/// rather than a copy.
/// Directories are compared by identity: the files inside them that matter
/// are watched on their own, and a directory's timestamps move on every index
/// or ref update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Fingerprint {
    Missing,
    Unreadable(io::ErrorKind),
    Directory(ObjectId),
    Contents {
        len: u64,
        digest: [u8; 32],
    },
    Oversized,
    /// A FIFO, device or socket, which Git could block on; refused.
    Special,
}

impl Fingerprint {
    pub(super) fn of(path: &Path) -> Self {
        Self::read(path).0
    }

    pub(super) fn is_directory(&self) -> bool {
        matches!(self, Self::Directory(_))
    }

    fn read(path: &Path) -> (Self, Option<Vec<u8>>) {
        let kind = match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => match directory_id(path) {
                Ok(id) => Self::Directory(id),
                Err(error) => Self::Unreadable(error.kind()),
            },
            Ok(metadata) if !metadata.is_file() => Self::Special,
            Ok(metadata) if metadata.len() > MAX_WATCHED_FILE_BYTES => Self::Oversized,
            Ok(_) => {
                return match read_regular_file(path) {
                    Ok(Ok(bytes)) => (Self::contents(&bytes), Some(bytes)),
                    Ok(Err(kind)) => (kind, None),
                    Err(error) => (Self::Unreadable(error.kind()), None),
                };
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.kind()),
        };
        (kind, None)
    }

    fn contents(bytes: &[u8]) -> Self {
        Self::Contents {
            len: bytes.len() as u64,
            digest: Sha256::digest(bytes).into(),
        }
    }

    pub(super) fn is_unverifiable(&self) -> bool {
        matches!(self, Self::Oversized | Self::Special)
    }
}

#[cfg(unix)]
fn directory_id(path: &Path) -> io::Result<ObjectId> {
    use std::os::unix::fs::MetadataExt;

    let metadata = fs::metadata(path)?;
    Ok((metadata.dev(), metadata.ino()))
}

#[cfg(windows)]
fn directory_id(path: &Path) -> io::Result<ObjectId> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, FILE_FLAG_BACKUP_SEMANTICS, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, GetFileInformationByHandle,
    };

    let directory = fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `directory` is a live handle and `information` is writable.
    if unsafe { GetFileInformationByHandle(directory.as_raw_handle(), &mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok((
        u64::from(information.dwVolumeSerialNumber),
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    ))
}

#[cfg(not(any(unix, windows)))]
fn directory_id(_path: &Path) -> io::Result<ObjectId> {
    Err(io::ErrorKind::Unsupported.into())
}

/// Reads a file that was regular when checked. The handle is checked again so
/// a swap to a FIFO cannot block the caller; that, or growth past the limit,
/// yields the fingerprint to report instead.
fn read_regular_file(path: &Path) -> io::Result<Result<Vec<u8>, Fingerprint>> {
    let mut options = fs::OpenOptions::new();
    options.read(true);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NONBLOCK);
    }
    let file = options.open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Ok(Err(Fingerprint::Special));
    }
    let mut bytes = Vec::new();
    file.take(MAX_WATCHED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WATCHED_FILE_BYTES {
        return Ok(Err(Fingerprint::Oversized));
    }
    Ok(Ok(bytes))
}
