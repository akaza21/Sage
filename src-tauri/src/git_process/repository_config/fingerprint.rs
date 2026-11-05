//! What a watched path held when an admission verdict was reached.

use std::{
    fs,
    io::{self, Read as _},
    path::{Path, PathBuf},
};

/// Larger watched files are refused rather than read on every call.
const MAX_WATCHED_FILE_BYTES: u64 = 256 * 1024;

pub(super) fn watch(path: PathBuf) -> (PathBuf, Fingerprint) {
    let fingerprint = Fingerprint::of(&path);
    (path, fingerprint)
}
/// What a watched path held when the verdict was reached. Regular files are
/// compared by their bytes, which no timestamp or size trick can forge.
/// Directories record only their kind: the files inside them that matter are
/// watched on their own, and a directory's timestamps move on every index or
/// ref update.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum Fingerprint {
    Missing,
    Unreadable(io::ErrorKind),
    Directory,
    Contents(Vec<u8>),
    /// Too large to re-read on every call; such a config is refused.
    Oversized,
    /// A FIFO, device or socket, which Git could block on; refused.
    Special,
    /// A symlink or reparse point, which could lead outside the root; refused.
    Link,
}

impl Fingerprint {
    pub(super) fn of(path: &Path) -> Self {
        match fs::symlink_metadata(path) {
            Ok(metadata) if is_link(&metadata) => return Self::Link,
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Self::Missing,
            Err(error) => return Self::Unreadable(error.kind()),
        }
        match fs::metadata(path) {
            Ok(metadata) if metadata.is_dir() => Self::Directory,
            Ok(metadata) if !metadata.is_file() => Self::Special,
            Ok(metadata) if metadata.len() > MAX_WATCHED_FILE_BYTES => Self::Oversized,
            Ok(_) => match read_regular_file(path) {
                Ok(fingerprint) => fingerprint,
                Err(error) => Self::Unreadable(error.kind()),
            },
            Err(error) if error.kind() == io::ErrorKind::NotFound => Self::Missing,
            Err(error) => Self::Unreadable(error.kind()),
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
/// a swap to a FIFO cannot block the caller.
fn read_regular_file(path: &Path) -> io::Result<Fingerprint> {
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
        return Ok(Fingerprint::Special);
    }
    let mut bytes = Vec::new();
    file.take(MAX_WATCHED_FILE_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_WATCHED_FILE_BYTES {
        return Ok(Fingerprint::Oversized);
    }
    Ok(Fingerprint::Contents(bytes))
}
