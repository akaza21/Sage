//! The empty file that reserves a silent download's name during planning.
//! Sage retires it after the registry claims the name, before WebView2 resumes.
//!
//! Sage only ever deletes a placeholder through a handle to the file it created, and
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
/// On Windows cleanup acquires a writer-excluding lease through this identity.
/// All Sage handles must close before the native download starts.
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

#[cfg(windows)]
fn reopen(file: &File, access: u32, sharing: u32) -> io::Result<File> {
    use std::os::windows::io::{AsRawHandle, FromRawHandle};
    use windows_sys::Win32::{Foundation::INVALID_HANDLE_VALUE, Storage::FileSystem::ReOpenFile};
    // SAFETY: the original handle is live. Reopening by handle preserves the
    // file identity without resolving its name again.
    let handle = unsafe { ReOpenFile(file.as_raw_handle(), access, sharing, 0) };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: ReOpenFile returned a new, uniquely owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

impl Placeholder {
    /// Retire our empty reservation before a path-only native handoff. A file
    /// that replaced it is preserved and prevents the download from starting.
    #[cfg(windows)]
    pub(crate) fn handoff(self) -> Option<PathBuf> {
        let path = self.path.clone();
        if !self.delete_if_empty() {
            return None;
        }
        // delete_if_empty consumed and closed every handle. A replacement or
        // an uncertain lookup refuses the handoff; nothing is removed by name.
        match std::fs::symlink_metadata(&path) {
            Err(error) if error.kind() == io::ErrorKind::NotFound => Some(path),
            _ => None,
        }
    }

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
        delete(self.file, &self.path)
    }
}

#[cfg(windows)]
fn delete(file: File, _: &Path) -> bool {
    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_READ_ATTRIBUTES, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };
    // Keep the identity while closing our creation handle's write access;
    // otherwise it would conflict with the writer-excluding cleanup lease.
    let Ok(identity) = reopen(
        &file,
        FILE_READ_ATTRIBUTES,
        FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
    ) else {
        return false;
    };
    drop(file);
    // Refuse cleanup while any writer is open, and prevent a writer from
    // changing the empty file between the size check and deletion.
    let Ok(deletion) = reopen(
        &identity,
        FILE_READ_ATTRIBUTES | DELETE,
        FILE_SHARE_READ | FILE_SHARE_DELETE,
    ) else {
        return false;
    };
    mark_empty(&deletion)
}

#[cfg(windows)]
fn mark_empty(file: &File) -> bool {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_DISPOSITION_INFO, FileDispositionInfo, SetFileInformationByHandle,
    };
    if file.metadata().map_or(true, |metadata| metadata.len() != 0) {
        return false;
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: this owned handle has DELETE access; the input points to a
    // FILE_DISPOSITION_INFO of the stated size. No path is reopened or removed.
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
fn delete(_: File, _: &Path) -> bool {
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

    #[cfg(windows)]
    #[test]
    fn handoff_removes_the_reservation_before_the_native_destination_is_created() {
        use std::{io::Write, os::windows::fs::OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;

        let directory = scratch("handoff");
        let path = directory.join("report-b.txt");
        let selected = create(&path).unwrap().handoff().unwrap();
        assert_eq!(selected, path);
        assert!(
            !path.exists(),
            "an abandoned handoff must leave no placeholder"
        );
        let mut native = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ)
            .open(&selected)
            .unwrap();
        native.write_all(b"bravo\n").unwrap();
        drop(native);
        assert_eq!(std::fs::read(&path).unwrap(), b"bravo\n");
        std::fs::remove_dir_all(directory).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn handoff_refuses_a_replacement_and_preserves_its_contents() {
        let directory = scratch("handoff-replaced");
        let path = directory.join("report-b.txt");
        let placeholder = create(&path).unwrap();
        std::fs::rename(&path, directory.join("reserved.bin")).unwrap();
        std::fs::write(&path, b"another file").unwrap();
        assert!(placeholder.handoff().is_none());
        assert_eq!(std::fs::read(&path).unwrap(), b"another file");
        std::fs::remove_dir_all(directory).unwrap();
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

    #[cfg(windows)]
    #[test]
    fn cleanup_keeps_an_empty_file_while_a_writer_can_still_fill_it() {
        use std::{io::Write, os::windows::fs::OpenOptionsExt};
        use windows_sys::Win32::Storage::FileSystem::{
            FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE,
        };
        let directory = scratch("active-writer");
        let path = directory.join("a.bin");
        let placeholder = create(&path).unwrap();
        let mut writer = OpenOptions::new()
            .write(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
            .open(&path)
            .unwrap();
        let deleted = placeholder.delete_if_empty();
        writer.write_all(b"downloaded").unwrap();
        drop(writer);
        assert!(!deleted);
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
