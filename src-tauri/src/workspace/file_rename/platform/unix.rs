use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        raw::{c_char, c_int, c_uint},
        unix::{ffi::OsStrExt, fs::OpenOptionsExt},
    },
    path::{Component, Path},
    sync::Arc,
};

use crate::{
    error::{AppError, AppResult},
    workspace::{RootIdentity, root_identity_metadata},
};

use super::{
    FileRenameAuthority, ParentAuthority, RenameIdentityProof, RenameObservation, observe_listing,
    verify_visible_authority,
};

#[cfg(target_os = "macos")]
const O_DIRECTORY: c_int = 0x10_0000;
#[cfg(target_os = "linux")]
const O_DIRECTORY: c_int = 0x1_0000;
#[cfg(target_os = "macos")]
const O_NOFOLLOW: c_int = 0x100;
#[cfg(target_os = "linux")]
const O_NOFOLLOW: c_int = 0x2_0000;
#[cfg(target_os = "macos")]
const O_CLOEXEC: c_int = 0x100_0000;
#[cfg(target_os = "linux")]
const O_CLOEXEC: c_int = 0x8_0000;
const O_RDONLY: c_int = 0;
#[cfg(target_os = "linux")]
const RENAME_NOREPLACE: c_uint = 1;
#[cfg(target_os = "macos")]
const RENAME_EXCL: c_uint = 4;

unsafe extern "C" {
    fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    #[cfg(target_os = "linux")]
    fn renameat2(
        olddirfd: c_int,
        oldpath: *const c_char,
        newdirfd: c_int,
        newpath: *const c_char,
        flags: c_uint,
    ) -> c_int;
    #[cfg(target_os = "macos")]
    fn renameatx_np(
        fromfd: c_int,
        from: *const c_char,
        tofd: c_int,
        to: *const c_char,
        flags: c_uint,
    ) -> c_int;
}

fn open_parent(authority: ParentAuthority<'_>) -> AppResult<Vec<File>> {
    let root = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        .open(authority.root)?;
    if root_identity_metadata(&root.metadata()?)? != authority.expected_root {
        return Err(AppError::InvalidRequest(
            "workspace root identity changed before file rename".into(),
        ));
    }
    let mut directories = vec![root];
    for component in Path::new(authority.relative_path).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(
                "file rename parent is not canonical".into(),
            ));
        };
        let name = CString::new(name.as_encoded_bytes())
            .map_err(|_| AppError::InvalidRequest("file parent contains a null byte".into()))?;
        // SAFETY: the held descriptor and NUL-terminated component are valid.
        let descriptor = unsafe {
            openat(
                directories
                    .last()
                    .expect("file rename root authority exists")
                    .as_raw_fd(),
                name.as_ptr(),
                O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
            )
        };
        if descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: openat returned a new owned descriptor.
        directories.push(unsafe { File::from_raw_fd(descriptor) });
    }
    Ok(directories)
}

fn open_file_at(directory: &File, name: &CString) -> AppResult<File> {
    // SAFETY: the held descriptor and NUL-terminated name are valid.
    let descriptor = unsafe {
        openat(
            directory.as_raw_fd(),
            name.as_ptr(),
            O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a new owned descriptor.
    let file = unsafe { File::from_raw_fd(descriptor) };
    if !file.metadata()?.is_file() {
        return Err(AppError::InvalidRequest(
            "file rename source must be one regular file".into(),
        ));
    }
    Ok(file)
}

fn relative_identity(directory: &File, name: &CString) -> AppResult<RootIdentity> {
    root_identity_metadata(&open_file_at(directory, name)?.metadata()?)
}

fn source_is_current(
    authority: &FileRenameAuthority,
    root: &Path,
    parent_relative: &str,
    source_name: &CString,
) -> AppResult<()> {
    verify_visible_authority(authority, root, parent_relative)?;
    if relative_identity(authority.parent(), source_name)? != authority.proof.source {
        return Err(AppError::InvalidRequest(
            "file rename source identity changed before mutation".into(),
        ));
    }
    Ok(())
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
    let directory = directories
        .last()
        .expect("file rename root/parent authority exists");
    let source_name = CString::new(source_name)
        .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
    let target_name = CString::new(target_name)
        .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
    let source = open_file_at(directory, &source_name)?;
    let proof = RenameIdentityProof {
        root: expected_root,
        parent: root_identity_metadata(&directory.metadata()?)?,
        source: root_identity_metadata(&source.metadata()?)?,
    };
    let authority = Arc::new(FileRenameAuthority {
        proof,
        directories,
        source,
    });
    source_is_current(&authority, root, parent_relative, &source_name)?;
    preflight()?;
    source_is_current(&authority, root, parent_relative, &source_name)?;
    // SAFETY: both names and the held parent descriptor are valid.
    let renamed = unsafe {
        #[cfg(target_os = "linux")]
        {
            renameat2(
                authority.parent().as_raw_fd(),
                source_name.as_ptr(),
                authority.parent().as_raw_fd(),
                target_name.as_ptr(),
                RENAME_NOREPLACE,
            )
        }
        #[cfg(target_os = "macos")]
        {
            renameatx_np(
                authority.parent().as_raw_fd(),
                source_name.as_ptr(),
                authority.parent().as_raw_fd(),
                target_name.as_ptr(),
                RENAME_EXCL,
            )
        }
    };
    if renamed < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    committed(Arc::clone(&authority));
    let warning = sync_parent(authority.parent())
        .err()
        .map(|_| "File was renamed, but its durability could not be confirmed.".to_owned());
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    let target_identity_confirmed = relative_identity(authority.parent(), &target_name)
        .is_ok_and(|target| target == authority.proof.source);
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
    let target_name = CString::new(target_name)
        .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
    let target_identity_confirmed = relative_identity(authority.parent(), &target_name)
        .is_ok_and(|target| target == authority.proof.source);
    let listing = observe_listing(&authority, root, parent_relative, list_after);
    Ok(RenameObservation {
        warning: None,
        listing,
        target_identity_confirmed,
    })
}
