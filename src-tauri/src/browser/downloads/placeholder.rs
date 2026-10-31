//! The empty file that reserves a silent download's name, held open by Sage
//! until the download settles.
//!
//! Sage only ever deletes a placeholder through the handle it created, and
//! only while that file is still empty, so it can never remove a different
//! file that took the name. On Windows the delete is
//! `SetFileInformationByHandle(FileDispositionInfo)` on that handle. Unix has
//! no delete-by-handle, so there an unused placeholder is left in place
//! rather than removed by name. The download handling itself is WebView2
//! code and runs on Windows; the Unix build keeps the shared code tested.
//!
//! After a crash nothing is deleted automatically: an empty placeholder can
//! be left behind, never a user's file removed.

use std::{
    fs::{File, OpenOptions},
    io,
    path::{Path, PathBuf},
};

/// A reserved name and the open handle of the empty file at it.
#[derive(Debug)]
pub(crate) struct Placeholder {
    pub(crate) path: PathBuf,
    file: File,
}

impl PartialEq for Placeholder {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Placeholder {}

/// Creates `path` as a new empty file, failing if anything exists there.
/// On Windows the handle may delete the file and shares read, write and
/// delete, so WebView2 can still write or rename onto the name.
pub(crate) fn create(path: &Path) -> io::Result<Placeholder> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::{
            DELETE, FILE_GENERIC_WRITE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ,
            FILE_SHARE_WRITE,
        };
        options
            .access_mode(FILE_GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE);
    }
    let file = options.open(path)?;
    Ok(Placeholder {
        path: path.to_owned(),
        file,
    })
}

impl Placeholder {
    /// Deletes the file through its handle if it is still empty. Returns
    /// whether it was deleted; anything uncertain keeps the file.
    pub(crate) fn delete_if_empty(self) -> bool {
        if self
            .file
            .metadata()
            .map_or(true, |metadata| metadata.len() != 0)
        {
            return false;
        }
        delete(&self.file, &self.path)
    }
}

#[cfg(windows)]
fn delete(file: &File, _: &Path) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle is live and was opened with DELETE access; the
    // input points to a FILE_DISPOSITION_INFO of the stated size. The file
    // is removed when this, the last Sage handle, closes.
    unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        ) != 0
    }
}

/// Without a delete-by-handle (Unix), nothing is deleted: removing by name
/// could remove a file another program put there after any check.
#[cfg(not(windows))]
fn delete(_: &File, _: &Path) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let directory = std::env::temp_dir().join(format!(
            "sage-download-placeholder-{name}-{}",
            uuid::Uuid::new_v4()
        ));
        std::fs::create_dir_all(&directory).unwrap();
        directory
    }

    #[test]
    fn an_empty_placeholder_is_deleted_through_its_handle_or_else_kept() {
        let directory = scratch("empty");
        let path = directory.join("a.bin");
        let placeholder = create(&path).unwrap();
        assert!(path.exists());
        let deleted = placeholder.delete_if_empty();
        // Windows deletes through the handle; Unix, with no such delete,
        // keeps the file rather than remove it by name.
        assert_eq!(deleted, cfg!(windows));
        assert_eq!(path.exists(), !cfg!(windows));
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn create_never_reuses_an_existing_name() {
        let directory = scratch("exists");
        let path = directory.join("a.bin");
        std::fs::write(&path, b"mine").unwrap();
        assert_eq!(
            create(&path).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(std::fs::read(&path).unwrap(), b"mine");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_placeholder_that_received_content_is_kept() {
        let directory = scratch("content");
        let path = directory.join("a.bin");
        let placeholder = create(&path).unwrap();
        std::fs::write(&path, b"downloaded").unwrap();
        assert!(!placeholder.delete_if_empty());
        assert_eq!(std::fs::read(&path).unwrap(), b"downloaded");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn a_file_that_replaced_the_placeholder_is_never_deleted() {
        let directory = scratch("replaced");
        let path = directory.join("a.bin");
        let placeholder = create(&path).unwrap();
        // Another program (or a download's final rename) puts a new, empty
        // file at the name while Sage still holds the old one open.
        let staged = directory.join("staged.bin");
        std::fs::write(&staged, b"").unwrap();
        std::fs::rename(&staged, &path).unwrap();
        placeholder.delete_if_empty();
        assert!(path.exists());
        std::fs::remove_dir_all(directory).unwrap();
    }
}
