use std::{fs::File, path::Path};

use crate::{domain::DirectoryListing, error::AppResult};

pub(super) struct ParentAuthority<'a> {
    pub(super) root: &'a Path,
    pub(super) expected_root: super::super::RootIdentity,
    pub(super) relative_path: &'a str,
}

pub(super) struct RenameObservation {
    pub(super) warning: Option<String>,
    pub(super) listing: AppResult<DirectoryListing>,
    pub(super) target_identity_confirmed: bool,
    pub(super) guards: Vec<File>,
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
mod unix {
    use std::{
        ffi::CString,
        fs::{File, OpenOptions},
        os::{
            fd::{AsRawFd, FromRawFd},
            raw::{c_char, c_int, c_uint},
            unix::ffi::OsStrExt,
            unix::fs::{MetadataExt, OpenOptionsExt},
        },
        path::{Component, Path},
    };

    use crate::error::{AppError, AppResult};

    use super::{ParentAuthority, RenameObservation};

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

    fn open_parent(authority: ParentAuthority<'_>) -> AppResult<File> {
        let mut directory = OpenOptions::new()
            .read(true)
            .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
            .open(authority.root)?;
        if super::super::super::super::root_identity_metadata(&directory.metadata()?)?
            != authority.expected_root
        {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before file rename".into(),
            ));
        }
        for component in Path::new(authority.relative_path).components() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file rename parent is not canonical".into(),
                ));
            };
            let name = CString::new(name.as_encoded_bytes())
                .map_err(|_| AppError::InvalidRequest("file parent contains a null byte".into()))?;
            // SAFETY: the held descriptor and NUL-terminated component are valid for the call.
            let descriptor = unsafe {
                openat(
                    directory.as_raw_fd(),
                    name.as_ptr(),
                    O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
                )
            };
            if descriptor < 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            // SAFETY: openat returned a new owned descriptor.
            directory = unsafe { File::from_raw_fd(descriptor) };
        }
        Ok(directory)
    }

    pub(in crate::workspace::file_rename) fn rename_file_noreplace<C, S, L>(
        authority: ParentAuthority<'_>,
        source_name: &str,
        target_name: &str,
        committed: C,
        sync_parent: S,
        list_after: L,
    ) -> AppResult<RenameObservation>
    where
        C: FnOnce(),
        S: FnOnce(&File) -> std::io::Result<()>,
        L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
    {
        let root = authority.root;
        let parent_relative = authority.relative_path;
        let directory = open_parent(authority)?;
        let source_name = CString::new(source_name)
            .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
        let target_name = CString::new(target_name)
            .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
        // SAFETY: the directory descriptor and source name remain valid for the call.
        let source_descriptor = unsafe {
            openat(
                directory.as_raw_fd(),
                source_name.as_ptr(),
                O_RDONLY | O_NOFOLLOW | O_CLOEXEC,
            )
        };
        if source_descriptor < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        // SAFETY: openat returned a new owned descriptor.
        let source = unsafe { File::from_raw_fd(source_descriptor) };
        if !source.metadata()?.is_file() {
            return Err(AppError::InvalidRequest(
                "file rename source must be one regular file".into(),
            ));
        }
        // SAFETY: both names and the held parent descriptor are valid for the syscall.
        let renamed = unsafe {
            #[cfg(target_os = "linux")]
            {
                renameat2(
                    directory.as_raw_fd(),
                    source_name.as_ptr(),
                    directory.as_raw_fd(),
                    target_name.as_ptr(),
                    RENAME_NOREPLACE,
                )
            }
            #[cfg(target_os = "macos")]
            {
                renameatx_np(
                    directory.as_raw_fd(),
                    source_name.as_ptr(),
                    directory.as_raw_fd(),
                    target_name.as_ptr(),
                    RENAME_EXCL,
                )
            }
        };
        if renamed < 0 {
            return Err(std::io::Error::last_os_error().into());
        }
        committed();
        let warning = sync_parent(&directory)
            .err()
            .map(|_| "File was renamed, but its durability could not be confirmed.".to_owned());
        let listing = list_after();
        let target = root
            .join(parent_relative)
            .join(std::ffi::OsStr::from_bytes(target_name.to_bytes()));
        let target_identity_confirmed = source
            .metadata()
            .ok()
            .zip(target.symlink_metadata().ok())
            .is_some_and(|(source, target)| {
                target.is_file() && source.dev() == target.dev() && source.ino() == target.ino()
            });
        Ok(RenameObservation {
            warning,
            listing,
            target_identity_confirmed,
            guards: vec![directory, source],
        })
    }
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) use unix::rename_file_noreplace;

#[cfg(windows)]
mod windows {
    use std::{
        fs::{File, OpenOptions},
        mem::{offset_of, size_of},
        os::windows::{
            fs::{MetadataExt, OpenOptionsExt},
            io::AsRawHandle,
        },
        path::{Component, Path},
        ptr,
    };

    use windows_sys::Win32::Storage::FileSystem::{
        DELETE, FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS,
        FILE_FLAG_OPEN_REPARSE_POINT, FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_RENAME_INFO,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FileRenameInfo,
        SetFileInformationByHandle,
    };

    use crate::{
        error::{AppError, AppResult},
        workspace::{root_identity_file, root_identity_path},
    };

    use super::{ParentAuthority, RenameObservation};

    fn open_directory(path: &Path) -> AppResult<File> {
        let directory = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE)
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

    fn open_parent(authority: ParentAuthority<'_>) -> AppResult<(Vec<File>, std::path::PathBuf)> {
        let root_guard = open_directory(authority.root)?;
        if root_identity_file(&root_guard)? != authority.expected_root {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before file rename".into(),
            ));
        }
        let mut guards = vec![root_guard];
        let mut directory = authority.root.to_path_buf();
        for component in Path::new(authority.relative_path).components() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "file rename parent is not canonical".into(),
                ));
            };
            directory.push(name);
            guards.push(open_directory(&directory)?);
        }
        Ok((guards, directory))
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
        let words = total.div_ceil(size_of::<usize>());
        let mut storage = vec![0_usize; words];
        let information = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
        // SAFETY: storage is aligned and sized for the header plus the exact UTF-16 name.
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
                u32::try_from(total).map_err(|_| {
                    AppError::ResourceLimit("file rename request is too large".into())
                })?,
            ) == 0
            {
                return Err(std::io::Error::last_os_error().into());
            }
        }
        Ok(())
    }

    pub(in crate::workspace::file_rename) fn rename_file_noreplace<C, S, L>(
        authority: ParentAuthority<'_>,
        source_name: &str,
        target_name: &str,
        committed: C,
        sync_parent: S,
        list_after: L,
    ) -> AppResult<RenameObservation>
    where
        C: FnOnce(),
        S: FnOnce(&File) -> std::io::Result<()>,
        L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
    {
        let (mut guards, parent_path) = open_parent(authority)?;
        let source = open_source(&parent_path.join(source_name))?;
        let source_identity = root_identity_file(&source)?;
        rename_held(&source, &parent_path.join(target_name))?;
        committed();
        let warning = sync_parent(guards.last().expect("file rename parent authority exists"))
            .err()
            .map(|_| "File was renamed, but its durability could not be confirmed.".to_owned());
        let listing = list_after();
        let target_identity_confirmed = root_identity_path(&parent_path.join(target_name))
            .is_ok_and(|target| target == source_identity);
        guards.push(source);
        Ok(RenameObservation {
            warning,
            listing,
            target_identity_confirmed,
            guards,
        })
    }
}

#[cfg(windows)]
pub(super) use windows::rename_file_noreplace;

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn rename_file_noreplace<C, S, L>(
    _authority: ParentAuthority<'_>,
    _source_name: &str,
    _target_name: &str,
    _committed: C,
    _sync_parent: S,
    _list_after: L,
) -> AppResult<RenameObservation>
where
    C: FnOnce(),
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<DirectoryListing>,
{
    Err(crate::error::AppError::InvalidRequest(
        "safe file rename is unavailable on this platform".into(),
    ))
}
