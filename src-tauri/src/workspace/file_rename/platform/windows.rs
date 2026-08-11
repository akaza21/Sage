use std::{
    fs::{File, OpenOptions},
    mem::{offset_of, size_of},
    os::windows::{
        fs::{MetadataExt, OpenOptionsExt},
        io::AsRawHandle,
    },
    path::{Component, Path},
    ptr,
    sync::Arc,
};

use windows_sys::Win32::Storage::FileSystem::{
    DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
    FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO, FILE_SHARE_DELETE,
    FILE_SHARE_READ, FILE_SHARE_WRITE, FileRenameInfo, SetFileInformationByHandle,
};

use crate::{
    error::{AppError, AppResult},
    workspace::root_identity_file,
};

use super::{
    FileRenameAuthority, ParentAuthority, RenameIdentityProof, RenameObservation, observe_listing,
    verify_visible_authority,
};

fn open_directory(path: &Path) -> AppResult<File> {
    let directory = OpenOptions::new()
        .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = directory.metadata()?;
    if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "file rename parent traverses a reparse point".into(),
        ));
    }
    Ok(directory)
}

fn open_parent(authority: ParentAuthority<'_>) -> AppResult<Vec<File>> {
    let root = open_directory(authority.root)?;
    if root_identity_file(&root)? != authority.expected_root {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before file rename".into(),
        ));
    }
    let mut directories = vec![root];
    let mut path = authority.root.to_path_buf();
    for component in Path::new(authority.relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(
                "file rename parent is not canonical".into(),
            ));
        };
        path.push(name);
        directories.push(open_directory(&path)?);
    }
    Ok(directories)
}

fn open_source(path: &Path) -> AppResult<File> {
    let source = OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = source.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "file rename source must be one regular non-reparse file".into(),
        ));
    }
    Ok(source)
}

fn rename_held(source: &File, target: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;

    let name = target.as_os_str().encode_wide().collect::<Vec<_>>();
    let name_bytes = name
        .len()
        .checked_mul(size_of::<u16>())
        .ok_or_else(|| AppError::ResourceLimit("file rename name is too large".into()))?;
    let total = offset_of!(FILE_RENAME_INFO, FileName)
        .checked_add(name_bytes)
        .ok_or_else(|| AppError::ResourceLimit("file rename request is too large".into()))?;
    let mut storage = vec![0_usize; total.div_ceil(size_of::<usize>())];
    let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: storage is aligned and sized for the header plus the UTF-16 name.
    unsafe {
        (*information).Anonymous.ReplaceIfExists = false;
        (*information).RootDirectory = std::ptr::null_mut();
        (*information).FileNameLength = u32::try_from(name_bytes)
            .map_err(|_| AppError::ResourceLimit("file rename name is too large".into()))?;
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            (*information).FileName.as_mut_ptr(),
            name.len(),
        );
        if SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            information.cast(),
            u32::try_from(total)
                .map_err(|_| AppError::ResourceLimit("file rename request is too large".into()))?,
        ) == 0
        {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}

fn parent_path(root: &Path, parent_relative: &str) -> std::path::PathBuf {
    if parent_relative.is_empty() {
        root.to_path_buf()
    } else {
        root.join(parent_relative)
    }
}

fn source_is_current(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
    source_name: &str,
) -> AppResult<()> {
    verify_visible_authority(authority, root, parent_relative)?;
    let source = open_source(&parent_path(root, parent_relative).join(source_name))?;
    if root_identity_file(&source)? != authority.proof.source {
        return Err(AppError::InvalidRequest(
            "file rename source identity changed before mutation".into(),
        ));
    }
    Ok(())
}

fn target_is_retained_source(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
    target_name: &str,
) -> bool {
    verify_visible_authority(authority, root, parent_relative).is_ok()
        && open_source(&parent_path(root, parent_relative).join(target_name))
            .and_then(|target| root_identity_file(&target))
            .is_ok_and(|target| target == authority.proof.source)
}

pub(in crate::workspace::file_rename) fn rename_file_noreplace<C, P, S, L>(
    authority: ParentAuthority<'_>,
    source_name: &str,
    target_name: &str,
    committed: C,
    preflight: P,
    sync_parent: S,
    list_after: L,
) -> AppResult<RenameObservation>
where
    C: FnOnce(Arc<FileRenameAuthority>),
    P: FnOnce() -> AppResult<()>,
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
{
    let root = authority.root;
    let expected_root = authority.expected_root;
    let parent_relative = authority.relative_path;
    let directories = open_parent(authority)?;
    let path = parent_path(root, parent_relative);
    let source = open_source(&path.join(source_name))?;
    let proof = RenameIdentityProof {
        root: expected_root,
        parent: root_identity_file(
            directories
                .last()
                .expect("file rename root/parent authority exists"),
        )?,
        source: root_identity_file(&source)?,
    };
    let authority = Arc::new(FileRenameAuthority {
        proof,
        directories,
        source,
    });
    source_is_current(&authority, root, parent_relative, source_name)?;
    preflight()?;
    source_is_current(&authority, root, parent_relative, source_name)?;
    verify_visible_authority(&authority, root, parent_relative)?;
    // Windows requires an absolute target here. All path components are held
    // without FILE_SHARE_DELETE and re-proven immediately before the call.
    rename_held(&authority.source, &path.join(target_name))?;
    committed(Arc::clone(&authority));
    let warning = sync_parent(authority.parent())
        .err()
        .map(|_| "File was renamed, but its durability could not be confirmed.".to_owned());
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    let target_identity_confirmed =
        target_is_retained_source(&authority, root, parent_relative, target_name);
    Ok(RenameObservation {
        warning,
        listing,
        target_identity_confirmed,
    })
}

pub(in crate::workspace::file_rename) fn observe_recovery<L>(
    authority: Arc<FileRenameAuthority>,
    root: &Path,
    parent_relative: &str,
    _source_name: &str,
    target_name: &str,
    list_after: L,
) -> AppResult<RenameObservation>
where
    L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
{
    verify_visible_authority(&authority, root, parent_relative)?;
    let target_identity_confirmed =
        target_is_retained_source(&authority, root, parent_relative, target_name);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(RenameObservation {
        warning: None,
        listing,
        target_identity_confirmed,
    })
}
