//! WebView2 may overwrite its destination. Only give it a file inside a
//! freshly created, unpredictable directory on the destination volume.
//! Publish the completed file with a single no-replace rename per candidate.

use std::{
    io,
    path::{Path, PathBuf},
};

#[derive(Debug)]
pub(super) struct Staging {
    directory: PathBuf,
    pub(super) path: PathBuf,
}

impl Staging {
    pub(super) fn create(folder: &Path) -> io::Result<Self> {
        let directory = folder.join(format!(".sage-download-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&directory)?;
        Ok(Self {
            path: directory.join("download"),
            directory,
        })
    }

    pub(super) fn publish(&self, folder: &Path, name: &str) -> io::Result<PathBuf> {
        for candidate in super::folder::destination_candidates(folder, name) {
            match move_without_replacing(&self.path, &candidate) {
                Ok(()) => return Ok(candidate),
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(error),
            }
        }
        Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "browser download names exhausted",
        ))
    }

    /// Only our fixed file and its private directory are removed. Never walk
    /// the directory or remove anything at a public destination by name.
    pub(super) fn cleanup(self) {
        let _ = std::fs::remove_file(&self.path);
        let _ = std::fs::remove_dir(&self.directory);
    }
}

fn move_without_replacing(from: &Path, to: &Path) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::MoveFileExW;

    let from = from
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let to = to
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    // SAFETY: both paths are NUL-terminated and live through the call. Zero
    // flags forbids replacement and cross-volume copy/delete fallbacks.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), 0) } == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
