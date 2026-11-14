//! The CSV an import read holds every password in plain text, so the page
//! offers to delete it afterwards. The file is pinned by the identity of the
//! handle the import read it through, and deletion removes it only through a
//! step that proves that identity and that nothing can redirect between the
//! check and the removal. A file saved over the same name since the import,
//! or changed in place, is left alone.

use std::{fs::File, path::PathBuf};

use super::{VaultError, VaultResult, file_identity::FileIdentity};

#[cfg(unix)]
mod unix;
#[cfg(unix)]
use unix::delete_if_same;
#[cfg(windows)]
mod windows;
#[cfg(windows)]
use windows::delete_if_same;

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

    /// Refuses unless the name still leads to the file the import read,
    /// unchanged.
    pub(super) fn delete(&self) -> VaultResult<()> {
        delete_if_same(&self.path, |file| self.check(file))
    }

    fn check(&self, file: &File) -> VaultResult<()> {
        if FileIdentity::of(file)? == self.identity {
            Ok(())
        } else {
            Err(VaultError::ImportedFileChanged)
        }
    }
}

#[cfg(test)]
mod tests;
