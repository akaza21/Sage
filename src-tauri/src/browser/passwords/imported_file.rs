//! The CSV an import read holds every password in plain text, so the page
//! offers to delete it afterwards. The file is pinned by the identity of the
//! handle the import read it through, and deletion opens the name again and
//! removes it only if that handle proves the same identity. A file saved
//! over the same name since the import, or changed in place, is left alone.

use std::{fs::File, io, path::PathBuf, time::SystemTime};

use super::{VaultError, VaultResult};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::{delete_if_same, volume_and_file_id};
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::{delete_if_same, volume_and_file_id};

/// A file's identity and the version of its contents the import read: the
/// volume and file id name the file whatever it is called, and the length
/// and last write time tell a rewrite in place apart from the import.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FileIdentity {
    volume: u64,
    file_id: u128,
    len: u64,
    modified: SystemTime,
}

impl FileIdentity {
    fn of(file: &File) -> io::Result<Self> {
        let metadata = file.metadata()?;
        let (volume, file_id) = volume_and_file_id(file, &metadata)?;
        Ok(Self {
            volume,
            file_id,
            len: metadata.len(),
            modified: metadata.modified()?,
        })
    }
}

/// The file the last import read, as it was when the import read it.
pub(super) struct ImportedFile {
    path: PathBuf,
    identity: FileIdentity,
}

impl ImportedFile {
    /// Opens `path` for import and pins the file the returned handle reads,
    /// so the identity can never belong to a different file than the one
    /// whose passwords were imported.
    pub(super) fn open(path: PathBuf) -> VaultResult<(File, Self)> {
        let file = File::open(&path)?;
        let identity = FileIdentity::of(&file)?;
        Ok((file, Self { path, identity }))
    }

    /// Deletes the file if its name still leads to the file the import read,
    /// unchanged. Otherwise refuses and leaves whatever is there in place.
    pub(super) fn delete(&self) -> VaultResult<()> {
        delete_if_same(&self.path, |file| {
            if FileIdentity::of(file)? == self.identity {
                Ok(())
            } else {
                Err(VaultError::ImportedFileChanged)
            }
        })
    }
}
