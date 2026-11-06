//! What a watched path held when an admission verdict was reached.

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

/// Like [`watch`], and also returns a regular file's bytes, read in the same
/// pass that fingerprinted them, for a caller that parses the file now. The
/// bytes are never retained in a verdict.
pub(super) fn watch_reading(path: PathBuf) -> ((PathBuf, Fingerprint), Option<Vec<u8>>) {
    let (fingerprint, bytes) = Fingerprint::read(&path);
    ((path, fingerprint), bytes)
}

/// What a watched path held when the verdict was reached. Regular files are
/// compared by their length and SHA-256 digest, which no timestamp or size
/// trick can forge, so a verdict keeps 40 bytes per file rather than a copy.
/// Directories record only their kind: the files inside them that matter are
/// watched on their own, and a directory's timestamps move on every index or
/// ref update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Fingerprint {
    Missing,
    Unreadable(io::ErrorKind),
    Directory,
    Contents {
        len: u64,
        digest: [u8; 32],
    },
    /// Too large to re-read on every call; such a config is refused.
    Oversized,
    /// A FIFO, device or socket, which Git could block on; refused.
    Special,
    /// A symlink or reparse point, which could lead outside the root; refused.
    Link,
}

impl Fingerprint {
    pub(super) fn of(path: &Path) -> Self {
        Self::read(path).0
    }

    fn read(path: &Path) -> (Self, Option<Vec<u8>>) {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_link(&metadata) => return (Self::Link, None),
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return (Self::Missing, None),
            Err(error) => return (Self::Unreadable(error.kind()), None),
        }
        let kind = match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::Directory,
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

/// A symlink, or on Windows any reparse point (junctions, mount points and
/// placeholders alike), since Sage cannot tell where one leads.
fn is_link(metadata: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

        if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return true;
        }
    }
    metadata.file_type().is_symlink()
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
        options.custom_flags(libc::O_NONBLOCK | libc::O_NOFOLLOW);
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
