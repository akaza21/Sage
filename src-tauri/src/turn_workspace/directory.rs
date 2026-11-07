//! Held, no-follow directory handles for fingerprinting a workspace's dirty
//! files. Each child is opened relative to its parent's handle, so replacing a
//! directory with a link after the walk cannot redirect a later read outside
//! the directory that was opened.

use std::{
    fs::File,
    path::{Component, Path},
};

pub(super) struct Directory {
    anchors: Vec<File>,
    #[cfg(windows)]
    path: std::path::PathBuf,
}

/// One entry's status, read without following a final link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct EntryStatus {
    pub(super) regular_file: bool,
    pub(super) len: u64,
    /// Modification time in nanoseconds since the Unix epoch.
    pub(super) modified_nanos: Option<u64>,
}

impl Directory {
    pub(super) fn root(path: &Path) -> Option<Self> {
        Some(Self {
            anchors: vec![platform::open_root(path).ok()?],
            #[cfg(windows)]
            path: path.to_path_buf(),
        })
    }

    pub(super) fn child(&self, relative: &Path) -> Option<Self> {
        self.child_result(relative).ok()
    }

    fn child_result(&self, relative: &Path) -> std::io::Result<Self> {
        let invalid = || std::io::Error::from(std::io::ErrorKind::InvalidInput);
        let mut child = Self {
            anchors: vec![self.last()?.try_clone()?],
            #[cfg(windows)]
            path: self.path.clone(),
        };
        for component in relative.components() {
            let Component::Normal(name) = component else {
                return Err(invalid());
            };
            #[cfg(windows)]
            child.path.push(name);
            #[cfg(windows)]
            let next = platform::open_root(&child.path)?;
            #[cfg(not(windows))]
            let next = platform::open_child(child.last()?, name)?;
            child.anchors.push(next);
        }
        Ok(child)
    }

    fn last(&self) -> std::io::Result<&File> {
        self.anchors
            .last()
            .ok_or_else(|| std::io::ErrorKind::InvalidInput.into())
    }

    pub(super) fn identity(&self) -> Option<[u8; 24]> {
        let file = self.anchors.last()?;
        crate::workspace::leaf_identity(file, &file.metadata().ok()?)
            .ok()
            .map(|identity| identity.stable_bytes())
    }

    /// The status of `relative`, `Some(None)` when it does not exist, or
    /// `None` when it cannot be read from the directory that was opened.
    pub(super) fn entry_status(&self, relative: &Path) -> Option<Option<EntryStatus>> {
        let parent_path = relative.parent()?;
        let parent = match self.child_result(parent_path) {
            Ok(parent) => parent,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Some(None),
            Err(_) => return None,
        };
        let status = match parent.status_of(relative.file_name()?) {
            Ok(status) => Some(status),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(_) => return None,
        };
        (self.child(parent_path)?.identity()? == parent.identity()?).then_some(status)
    }

    fn status_of(&self, name: &std::ffi::OsStr) -> std::io::Result<EntryStatus> {
        #[cfg(windows)]
        {
            platform::status(&self.path.join(name))
        }
        #[cfg(not(windows))]
        {
            platform::status_at(self.last()?, name)
        }
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod platform {
    use std::{
        ffi::{CString, OsStr},
        fs::File,
        os::{
            fd::{AsRawFd, FromRawFd},
            unix::ffi::OsStrExt,
        },
        path::Path,
    };

    use super::EntryStatus;

    const NANOS_PER_SECOND: u64 = 1_000_000_000;

    pub(super) fn open_root(path: &Path) -> std::io::Result<File> {
        open_directory(libc::AT_FDCWD, path.as_os_str())
    }

    pub(super) fn open_child(parent: &File, name: &OsStr) -> std::io::Result<File> {
        open_directory(parent.as_raw_fd(), name)
    }

    /// Opens a directory relative to `base`, refusing a final symbolic link.
    fn open_directory(base: libc::c_int, name: &OsStr) -> std::io::Result<File> {
        let name = CString::new(name.as_bytes())?;
        // SAFETY: openat reads a NUL-terminated name and returns a new descriptor.
        let fd = unsafe {
            libc::openat(
                base,
                name.as_ptr(),
                libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            )
        };
        if fd < 0 {
            return Err(std::io::Error::last_os_error());
        }
        // SAFETY: openat returned a descriptor that nothing else owns.
        Ok(unsafe { File::from_raw_fd(fd) })
    }

    pub(super) fn status_at(parent: &File, name: &OsStr) -> std::io::Result<EntryStatus> {
        let name = CString::new(name.as_bytes())?;
        // SAFETY: an all-zero stat is a valid value for fstatat to overwrite.
        let mut stat: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: fstatat reads a NUL-terminated name and writes one stat.
        if unsafe {
            libc::fstatat(
                parent.as_raw_fd(),
                name.as_ptr(),
                &mut stat,
                libc::AT_SYMLINK_NOFOLLOW,
            )
        } < 0
        {
            return Err(std::io::Error::last_os_error());
        }
        Ok(EntryStatus {
            regular_file: stat.st_mode & libc::S_IFMT == libc::S_IFREG,
            len: u64::try_from(stat.st_size)
                .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidData))?,
            modified_nanos: u64::try_from(stat.st_mtime)
                .ok()
                .and_then(|seconds| seconds.checked_mul(NANOS_PER_SECOND))
                .zip(u64::try_from(stat.st_mtime_nsec).ok())
                .and_then(|(seconds, nanos)| seconds.checked_add(nanos)),
        })
    }
}

#[cfg(windows)]
mod platform {
    use std::{
        fs::{File, OpenOptions},
        path::Path,
        time::UNIX_EPOCH,
    };

    use super::EntryStatus;

    /// Windows has no handle-relative open in std; each component is opened
    /// by path and held without delete sharing, so it cannot be renamed or
    /// replaced while the walk holds it.
    pub(super) fn open_root(path: &Path) -> std::io::Result<File> {
        use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
            FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let file = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = file.metadata()?;
        if metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0 {
            Ok(file)
        } else {
            Err(std::io::ErrorKind::InvalidInput.into())
        }
    }

    pub(super) fn status(path: &Path) -> std::io::Result<EntryStatus> {
        let metadata = std::fs::symlink_metadata(path)?;
        Ok(EntryStatus {
            regular_file: metadata.is_file(),
            len: metadata.len(),
            modified_nanos: metadata
                .modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .and_then(|elapsed| u64::try_from(elapsed.as_nanos()).ok()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queued_directory_substitution_is_rejected_or_stays_pinned() {
        let root = std::env::temp_dir().join(format!("sage-anchor-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("child")).unwrap();
        let anchor = Directory::root(&root).unwrap();
        let child = anchor.child(Path::new("child")).unwrap();
        let identity = child.identity();
        let renamed = std::fs::rename(root.join("child"), root.join("old"));
        if renamed.is_ok() {
            std::fs::create_dir(root.join("child")).unwrap();
            assert_ne!(
                anchor.child(Path::new("child")).unwrap().identity(),
                identity
            );
            assert_eq!(child.identity(), identity);
        } else {
            assert_eq!(
                anchor.child(Path::new("child")).unwrap().identity(),
                identity
            );
        }
        drop(child);
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn symlink_substitution_never_enumerates_the_target() {
        let root = std::env::temp_dir().join(format!("sage-anchor-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("outside")).unwrap();
        let anchor = Directory::root(&root).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("child")).unwrap();
        assert!(anchor.child(Path::new("child")).is_none());
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn fingerprinting_after_enumeration_cannot_read_a_substituted_parent() {
        let root =
            std::env::temp_dir().join(format!("sage-fingerprint-link-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(root.join("child")).unwrap();
        std::fs::create_dir_all(root.join("outside")).unwrap();
        std::fs::write(root.join("child/file"), "original").unwrap();
        std::fs::write(
            root.join("outside/file"),
            "external target with a different size",
        )
        .unwrap();
        let anchor = Directory::root(&root).unwrap();
        let child = anchor.child(Path::new("child")).unwrap();
        let file = |directory: &Directory, path: &str| {
            directory
                .entry_status(Path::new(path))
                .map(|status| status.unwrap().len)
        };
        assert_eq!(file(&anchor, "child/file"), Some(8));
        std::fs::rename(root.join("child"), root.join("old")).unwrap();
        std::os::unix::fs::symlink(root.join("outside"), root.join("child")).unwrap();
        assert_eq!(file(&anchor, "child/file"), None);
        assert_eq!(file(&child, "file"), Some(8));
        drop(child);
        drop(anchor);
        std::fs::remove_dir_all(root).unwrap();
    }
}
