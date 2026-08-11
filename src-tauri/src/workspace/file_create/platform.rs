#[cfg(any(target_os = "linux", target_os = "macos"))]
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    os::{
        fd::{AsRawFd, FromRawFd},
        raw::{c_char, c_int},
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Component, Path},
};

#[cfg(any(target_os = "linux", target_os = "macos"))]
use super::ensure_root_identity;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::error::{AppError, AppResult};

pub(super) struct ParentAuthority<'a> {
    pub(super) root: &'a std::path::Path,
    pub(super) expected_root: super::super::RootIdentity,
    pub(super) relative_path: &'a str,
}

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
#[cfg(target_os = "macos")]
const O_CREAT: c_int = 0x200;
#[cfg(target_os = "linux")]
const O_CREAT: c_int = 0x40;
#[cfg(target_os = "macos")]
const O_EXCL: c_int = 0x800;
#[cfg(target_os = "linux")]
const O_EXCL: c_int = 0x80;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const O_RDONLY: c_int = 0;
#[cfg(any(target_os = "linux", target_os = "macos"))]
const O_WRONLY: c_int = 1;

#[cfg(any(target_os = "linux", target_os = "macos"))]
unsafe extern "C" {
    fn openat(dirfd: c_int, path: *const c_char, flags: c_int, ...) -> c_int;
    #[cfg(target_os = "linux")]
    fn mkdirat(dirfd: c_int, path: *const c_char, mode: std::os::raw::c_uint) -> c_int;
    #[cfg(target_os = "macos")]
    fn mkdirat(dirfd: c_int, path: *const c_char, mode: u16) -> c_int;
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
fn open_parent(
    root: &Path,
    expected_root: super::super::RootIdentity,
    parent: &str,
) -> AppResult<File> {
    let mut directory = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC)
        .open(root)?;
    ensure_root_identity(&directory.metadata()?, expected_root)?;
    for component in Path::new(parent).components() {
        let Component::Normal(name) = component else {
            return Err(AppError::InvalidRequest(
                "file parent is not canonical".into(),
            ));
        };
        let name = CString::new(name.as_encoded_bytes())
            .map_err(|_| AppError::InvalidRequest("file parent contains a null byte".into()))?;
        // SAFETY: the directory descriptor and NUL-terminated name remain valid for the call.
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
        // SAFETY: openat returned a new owned descriptor on success.
        directory = unsafe { File::from_raw_fd(descriptor) };
    }
    Ok(directory)
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_new<B, S, L>(
    root: &Path,
    expected_root: super::super::RootIdentity,
    parent: &str,
    basename: &str,
    before_create: B,
    sync_file: S,
    list_after: L,
) -> AppResult<(
    Option<String>,
    AppResult<crate::domain::DirectoryListing>,
    bool,
)>
where
    B: FnOnce(&Path) -> std::io::Result<()>,
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
{
    let directory = open_parent(root, expected_root, parent)?;
    before_create(&root.join(parent))?;
    let target = root.join(parent).join(basename);
    let basename = CString::new(basename)
        .map_err(|_| AppError::InvalidRequest("file name contains a null byte".into()))?;
    #[cfg(target_os = "macos")]
    let mode = 0o666 as c_int;
    #[cfg(target_os = "linux")]
    let mode = 0o666 as std::os::raw::c_uint;
    // SAFETY: the directory descriptor and NUL-terminated basename remain valid for the call.
    let descriptor = unsafe {
        openat(
            directory.as_raw_fd(),
            basename.as_ptr(),
            O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
            mode,
        )
    };
    if descriptor < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a new owned descriptor on success.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let warning = sync_file(&file)
        .err()
        .map(|_| "File was created, but its durability could not be confirmed.".to_owned());
    let listing = list_after();
    let resource_confirmed = file
        .metadata()
        .ok()
        .zip(target.symlink_metadata().ok())
        .is_some_and(|(created, target)| {
            target.is_file() && target.dev() == created.dev() && target.ino() == created.ino()
        });
    Ok((warning, listing, resource_confirmed))
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
pub(super) fn create_new_directory<B, C, S, L>(
    parent: ParentAuthority<'_>,
    basename: &str,
    before_create: B,
    created: C,
    sync_parent: S,
    list_after: L,
) -> AppResult<(
    Option<String>,
    AppResult<crate::domain::DirectoryListing>,
    bool,
)>
where
    B: FnOnce(&Path) -> std::io::Result<()>,
    C: FnOnce(),
    S: FnOnce(&File) -> std::io::Result<()>,
    L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
{
    let directory = open_parent(parent.root, parent.expected_root, parent.relative_path)?;
    before_create(&parent.root.join(parent.relative_path))?;
    let target = parent.root.join(parent.relative_path).join(basename);
    let basename = CString::new(basename)
        .map_err(|_| AppError::InvalidRequest("folder name contains a null byte".into()))?;
    #[cfg(target_os = "macos")]
    let mode = 0o777_u16;
    #[cfg(target_os = "linux")]
    let mode = 0o777 as std::os::raw::c_uint;
    // SAFETY: the held parent descriptor and NUL-terminated basename are valid for this call.
    if unsafe { mkdirat(directory.as_raw_fd(), basename.as_ptr(), mode) } < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    created();
    let warning = sync_parent(&directory)
        .err()
        .map(|_| "Folder was created, but its durability could not be confirmed.".to_owned());
    let listing = list_after();
    // SAFETY: openat returns a new descriptor, or a negative error value.
    let descriptor = unsafe {
        openat(
            directory.as_raw_fd(),
            basename.as_ptr(),
            O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC,
        )
    };
    let resource_confirmed = if descriptor < 0 {
        false
    } else {
        // SAFETY: openat returned a new owned descriptor on success.
        let created_directory = unsafe { File::from_raw_fd(descriptor) };
        created_directory
            .metadata()
            .ok()
            .zip(target.symlink_metadata().ok())
            .is_some_and(|(created, target)| {
                target.is_dir() && target.dev() == created.dev() && target.ino() == created.ino()
            })
    };
    Ok((warning, listing, resource_confirmed))
}

#[cfg(all(test, target_os = "macos"))]
pub(super) fn assert_apple_openat_variadic_binding() {
    let _: unsafe extern "C" fn(c_int, *const c_char, c_int, ...) -> c_int = openat;
}

#[cfg(all(test, target_os = "macos"))]
pub(super) fn assert_apple_mkdirat_binding() {
    let _: unsafe extern "C" fn(c_int, *const c_char, u16) -> c_int = mkdirat;
}

#[cfg(windows)]
mod windows {
    use std::{
        fs::{File, OpenOptions},
        os::windows::fs::{MetadataExt, OpenOptionsExt},
        path::{Component, Path},
    };
    use windows_sys::Win32::Storage::FileSystem::{
        FILE_ATTRIBUTE_REPARSE_POINT, FILE_FLAG_BACKUP_SEMANTICS, FILE_FLAG_OPEN_REPARSE_POINT,
        FILE_LIST_DIRECTORY, FILE_READ_ATTRIBUTES, FILE_SHARE_READ, FILE_SHARE_WRITE,
    };

    use crate::error::{AppError, AppResult};

    fn open_directory(path: &Path) -> AppResult<File> {
        let directory = OpenOptions::new()
            .access_mode(FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES)
            .share_mode(FILE_SHARE_READ)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        let metadata = directory.metadata()?;
        if !metadata.is_dir() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
            return Err(AppError::InvalidRequest(
                "file parent traverses a reparse point".into(),
            ));
        }
        Ok(directory)
    }

    fn open_parent(
        root: &Path,
        expected_root: super::super::super::RootIdentity,
        parent: &str,
    ) -> AppResult<(Vec<File>, std::path::PathBuf)> {
        let root_guard = open_directory(root)?;
        if super::super::super::root_identity_file(&root_guard)? != expected_root {
            return Err(AppError::InvalidRequest(
                "workspace root identity changed before resource creation".into(),
            ));
        }
        let mut guards = vec![root_guard];
        let mut directory = root.to_path_buf();
        for component in Path::new(parent).components() {
            let Component::Normal(name) = component else {
                return Err(AppError::InvalidRequest(
                    "resource parent is not canonical".into(),
                ));
            };
            directory.push(name);
            guards.push(open_directory(&directory)?);
        }
        Ok((guards, directory))
    }

    pub(crate) fn create_new<B, S, L>(
        root: &Path,
        expected_root: super::super::super::RootIdentity,
        parent: &str,
        basename: &str,
        before_create: B,
        sync_file: S,
        list_after: L,
    ) -> AppResult<(
        Option<String>,
        AppResult<crate::domain::DirectoryListing>,
        bool,
    )>
    where
        B: FnOnce(&Path) -> std::io::Result<()>,
        S: FnOnce(&File) -> std::io::Result<()>,
        L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
    {
        let (guards, directory) = open_parent(root, expected_root, parent)?;
        before_create(&directory)?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
            .open(directory.join(basename))?;
        let warning = sync_file(&file)
            .err()
            .map(|_| "File was created, but its durability could not be confirmed.".to_owned());
        let listing = list_after();
        drop(guards);
        Ok((warning, listing, true))
    }

    pub(crate) fn create_new_directory<B, C, S, L>(
        parent: super::ParentAuthority<'_>,
        basename: &str,
        before_create: B,
        created: C,
        sync_parent: S,
        list_after: L,
    ) -> AppResult<(
        Option<String>,
        AppResult<crate::domain::DirectoryListing>,
        bool,
    )>
    where
        B: FnOnce(&Path) -> std::io::Result<()>,
        C: FnOnce(),
        S: FnOnce(&File) -> std::io::Result<()>,
        L: FnOnce() -> AppResult<crate::domain::DirectoryListing>,
    {
        let (guards, directory) =
            open_parent(parent.root, parent.expected_root, parent.relative_path)?;
        before_create(&directory)?;
        let target = directory.join(basename);
        std::fs::create_dir(&target)?;
        created();
        let warning = sync_parent(guards.last().expect("parent authority exists"))
            .err()
            .map(|_| "Folder was created, but its durability could not be confirmed.".to_owned());
        let listing = list_after();
        let resource_confirmed = open_directory(&target).is_ok();
        drop(guards);
        Ok((warning, listing, resource_confirmed))
    }
}

#[cfg(windows)]
pub(super) use windows::{create_new, create_new_directory};

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn create_new<B, S, L>(
    _root: &std::path::Path,
    _expected_root: super::super::RootIdentity,
    _parent: &str,
    _basename: &str,
    _before_create: B,
    _sync_file: S,
    _list_after: L,
) -> crate::error::AppResult<(
    Option<String>,
    crate::error::AppResult<crate::domain::DirectoryListing>,
    bool,
)>
where
    B: FnOnce(&std::path::Path) -> std::io::Result<()>,
    S: FnOnce(&std::fs::File) -> std::io::Result<()>,
    L: FnOnce() -> crate::error::AppResult<crate::domain::DirectoryListing>,
{
    Err(crate::error::AppError::InvalidRequest(
        "safe file creation is unavailable on this platform".into(),
    ))
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub(super) fn create_new_directory<B, C, S, L>(
    _parent: ParentAuthority<'_>,
    _basename: &str,
    _before_create: B,
    _created: C,
    _sync_parent: S,
    _list_after: L,
) -> crate::error::AppResult<(
    Option<String>,
    crate::error::AppResult<crate::domain::DirectoryListing>,
    bool,
)>
where
    B: FnOnce(&std::path::Path) -> std::io::Result<()>,
    C: FnOnce(),
    S: FnOnce(&std::fs::File) -> std::io::Result<()>,
    L: FnOnce() -> crate::error::AppResult<crate::domain::DirectoryListing>,
{
    Err(crate::error::AppError::InvalidRequest(
        "safe folder creation is unavailable on this platform".into(),
    ))
}
