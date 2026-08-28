use std::sync::Arc;

#[cfg(windows)]
use std::{
    fs::{self, File, OpenOptions},
    mem::{offset_of, size_of},
    os::windows::{
        ffi::OsStrExt,
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::Path,
    ptr,
};

use crate::{
    domain::{DirectoryListing, ResourceRef},
    error::{AppError, AppResult},
};

use super::{FileRenameCommit, FileRenameMarker, Workspace};

#[cfg(windows)]
use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_READ, FileRenameInfo,
    SetFileInformationByHandle,
};

impl Workspace {
    #[cfg(test)]
    pub(crate) fn rename_empty_folder(
        &self,
        source: &ResourceRef,
        basename: &str,
    ) -> AppResult<FileRenameCommit> {
        self.rename_empty_folder_with(
            source,
            basename,
            Arc::new(FileRenameMarker::default()),
            || Ok(()),
            |parent| self.list_directory(parent),
        )
    }

    #[cfg(test)]
    pub(crate) fn rename_empty_folder_with_hook<P>(
        &self,
        source: &ResourceRef,
        basename: &str,
        before_rename: P,
    ) -> AppResult<FileRenameCommit>
    where
        P: FnOnce() -> AppResult<()>,
    {
        self.rename_empty_folder_with(
            source,
            basename,
            Arc::new(FileRenameMarker::default()),
            before_rename,
            |parent| self.list_directory(parent),
        )
    }

    pub(crate) fn rename_empty_folder_observed(
        &self,
        source: &ResourceRef,
        basename: &str,
        marker: Arc<FileRenameMarker>,
    ) -> AppResult<FileRenameCommit> {
        self.rename_empty_folder_with(
            source,
            basename,
            marker,
            || Ok(()),
            |parent| self.list_directory(parent),
        )
    }

    fn rename_empty_folder_with<P, L>(
        &self,
        source: &ResourceRef,
        basename: &str,
        marker: Arc<FileRenameMarker>,
        before_rename: P,
        list_after: L,
    ) -> AppResult<FileRenameCommit>
    where
        P: FnOnce() -> AppResult<()>,
        L: FnOnce(&ResourceRef) -> AppResult<DirectoryListing>,
    {
        #[cfg(not(windows))]
        {
            let _ = (source, basename, marker, before_rename, list_after);
            return Err(AppError::Unsupported(
                "empty-folder rename is currently supported only on Windows".into(),
            ));
        }
        #[cfg(windows)]
        {
            let source_path = self.validate_resource(source, false)?;
            if source_path.is_empty() {
                return Err(AppError::InvalidRequest(
                    "folder rename source must not be the Project root".into(),
                ));
            }
            let (parent_path, source_name) = source_path
                .rsplit_once('/')
                .map_or(("", source_path.as_str()), |(parent, name)| (parent, name));
            let target = self.rename_file_target(source, basename)?;
            let parent = ResourceRef {
                root_id: self.root_id().into(),
                relative_path: parent_path.into(),
            };
            let source_path = self.root().join(&source_path);
            let source_handle = hold_empty_directory(&source_path)?;

            let before = self.list_directory(&parent)?;
            if before.truncated {
                return Err(AppError::ResourceLimit(
                    "folder rename requires a complete parent listing".into(),
                ));
            }
            if !before.entries.iter().any(|entry| {
                entry.name == source_name && entry.resource == *source && entry.kind == "directory"
            }) {
                return Err(AppError::InvalidRequest(
                    "folder rename source must be one exact non-reparse directory".into(),
                ));
            }

            before_rename()?;
            rename_held_directory(&source_handle, &self.root().join(&target.relative_path))?;
            marker.mark_committed();
            let parent_listing = list_after(&parent).ok().filter(|listing| {
                !listing.truncated
                    && listing.entries.iter().any(|entry| {
                        entry.resource == target
                            && entry.kind == "directory"
                            && entry.byte_len.is_none()
                    })
                    && !listing
                        .entries
                        .iter()
                        .any(|entry| entry.resource == *source)
            });
            let resource_confirmed = parent_listing.is_some();
            Ok(FileRenameCommit {
                source: source.clone(),
                target,
                resource_confirmed,
                parent_listing,
                warning: (!resource_confirmed).then(|| {
                    "Folder rename completed, but Explorer could not confirm the exact directory identity; Explorer is refreshing."
                        .into()
                }),
            })
        }
    }
}

#[cfg(windows)]
fn hold_empty_directory(path: &Path) -> AppResult<File> {
    // Retain DELETE on the source while sharing read-only access. This holds
    // source replacement while the empty-directory check is committed below.
    let directory = hold_directory(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "folder rename source must be one exact non-reparse directory".into(),
        ));
    }
    if fs::read_dir(path)?.next().is_some() {
        return Err(AppError::InvalidRequest(
            "folder rename source must be an empty directory".into(),
        ));
    }
    Ok(directory)
}

#[cfg(windows)]
fn hold_directory(path: &Path) -> AppResult<File> {
    Ok(OpenOptions::new()
        .access_mode(DELETE | FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?)
}

#[cfg(windows)]
fn rename_held_directory(source: &File, target: &Path) -> AppResult<()> {
    let name = target.as_os_str().encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| AppError::ResourceLimit("folder rename name is too large".into()))?;
    // Windows consumes a terminated target name for directory handles even
    // though FileNameLength excludes that terminator.
    let storage_bytes = name_bytes
        .checked_add(size_of::<u16>())
        .ok_or_else(|| AppError::ResourceLimit("folder rename request is too large".into()))?;
    let total = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(storage_bytes)
        .ok_or_else(|| AppError::ResourceLimit("folder rename request is too large".into()))?;
    let mut storage = vec![0_usize; total.div_ceil(size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: storage is aligned and sized for the header plus the UTF-16 name.
    unsafe {
        (*information).Anonymous.ReplaceIfExists = false;
        (*information).RootDirectory = ptr::null_mut();
        (*information).FileNameLength = u32::try_from(name_bytes)
            .map_err(|_| AppError::ResourceLimit("folder rename name is too large".into()))?;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.len(),
        );
        if SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            information.cast(),
            u32::try_from(total).map_err(|_| {
                AppError::ResourceLimit("folder rename request is too large".into())
            })?,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}
