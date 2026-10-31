//! Which file a placeholder is, beyond its path: the volume and file index
//! on Windows (`GetFileInformationByHandle`), the device and inode on Unix.
//! Releasing a placeholder removes the file only while it is still the very
//! file Sage created and still empty, so a user's own file that took its
//! place, or a download WebView2 renamed onto the path, is never removed.

use std::{
    fs::{File, OpenOptions},
    io::ErrorKind,
    path::Path,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct FileIdentity {
    pub(crate) volume: u64,
    pub(crate) index: u64,
}

#[cfg(unix)]
pub(crate) fn of_file(file: &File) -> Option<FileIdentity> {
    use std::os::unix::fs::MetadataExt;
    let metadata = file.metadata().ok()?;
    Some(FileIdentity {
        volume: metadata.dev(),
        index: metadata.ino(),
    })
}

#[cfg(windows)]
pub(crate) fn of_file(file: &File) -> Option<FileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;
    let mut information = std::mem::MaybeUninit::uninit();
    // SAFETY: the file handle is live and the output points to writable storage.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), information.as_mut_ptr()) } == 0 {
        return None;
    }
    // SAFETY: GetFileInformationByHandle initialized the output on success.
    let information = unsafe { information.assume_init() };
    Some(FileIdentity {
        volume: u64::from(information.dwVolumeSerialNumber),
        index: (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow),
    })
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn of_file(_: &File) -> Option<FileIdentity> {
    None
}

/// What releasing a placeholder found.
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum Release {
    /// Removed, or already gone: nothing of Sage's is left.
    Done,
    /// Something else is there now (another file, content, a link): kept,
    /// and no longer Sage's to remove.
    NotOurs,
    /// Could not tell (the folder is unreachable, a read failed): kept, try
    /// again later.
    Unknown,
}

/// Removes `path` only if it is a regular, empty file with `identity`.
pub(crate) fn release_if_ours(path: &Path, identity: FileIdentity) -> Release {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Release::Done,
        Err(_) => return Release::Unknown,
    };
    if !metadata.is_file() || metadata.len() != 0 {
        return Release::NotOurs;
    }
    let Ok(file) = OpenOptions::new().read(true).open(path) else {
        return Release::Unknown;
    };
    match of_file(&file) {
        Some(found) if found != identity => return Release::NotOurs,
        Some(_) => {}
        None => return Release::Unknown,
    }
    if file.metadata().map(|metadata| metadata.len()).unwrap_or(1) != 0 {
        return Release::NotOurs;
    }
    drop(file);
    match std::fs::remove_file(path) {
        Ok(()) => Release::Done,
        Err(error) if error.kind() == ErrorKind::NotFound => Release::Done,
        Err(_) => Release::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> std::path::PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sage-download-identity-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    fn create(path: &Path) -> FileIdentity {
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(path)
            .unwrap();
        of_file(&file).unwrap()
    }

    #[test]
    fn our_empty_placeholder_is_removed() {
        let directory = scratch("ours");
        let path = directory.join("a.bin");
        let identity = create(&path);
        assert_eq!(release_if_ours(&path, identity), Release::Done);
        assert!(!path.exists());
        assert_eq!(release_if_ours(&path, identity), Release::Done);
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_users_empty_file_that_replaced_the_placeholder_is_kept() {
        let directory = scratch("replaced");
        let path = directory.join("a.bin");
        let identity = create(&path);
        // Keep the old inode alive elsewhere so the new file cannot reuse it.
        let moved = directory.join("moved.bin");
        std::fs::rename(&path, &moved).unwrap();
        create(&path);
        assert_eq!(release_if_ours(&path, identity), Release::NotOurs);
        assert!(path.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn our_placeholder_with_content_is_kept() {
        let directory = scratch("content");
        let path = directory.join("a.bin");
        let identity = create(&path);
        std::fs::write(&path, b"downloaded").unwrap();
        assert_eq!(release_if_ours(&path, identity), Release::NotOurs);
        assert_eq!(std::fs::read(&path).unwrap(), b"downloaded");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn an_unreachable_placeholder_is_unknown_and_kept_for_later() {
        let directory = scratch("unreachable");
        let path = directory.join("a.bin");
        let identity = create(&path);
        // A path through a regular file fails with neither NotFound nor a
        // file: the stand-in for an unreachable share.
        let blocked = path.join("child");
        assert_eq!(release_if_ours(&blocked, identity), Release::Unknown);
        assert!(path.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
