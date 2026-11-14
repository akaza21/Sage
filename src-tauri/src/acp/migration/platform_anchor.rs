use std::{
    fs::{File, OpenOptions},
    path::Path,
};

use sage_core::error::{AppError, AppResult};

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(super) struct NativeFileIdentity {
    pub(super) volume_serial: u32,
    pub(super) file_index: u64,
}

#[derive(Debug)]
pub(super) struct SqlitePathAnchor {
    file: File,
    identity: NativeFileIdentity,
}

impl SqlitePathAnchor {
    pub(super) fn identity(&self) -> &NativeFileIdentity {
        &self.identity
    }

    pub(super) fn verify_path(&self, path: &Path) -> AppResult<()> {
        let actual = file_identity(&open_sqlite_anchor(path, false, false)?)?;
        if actual != self.identity {
            return Err(AppError::InvalidRequest(
                "SQLite migration path changed after it was anchored".into(),
            ));
        }
        Ok(())
    }

    pub(super) fn sync(&self) -> AppResult<()> {
        self.file.sync_all()?;
        Ok(())
    }
}

pub(super) fn anchor_existing_sqlite(path: &Path, writable: bool) -> AppResult<SqlitePathAnchor> {
    let file = open_sqlite_anchor(path, writable, false)?;
    let identity = file_identity(&file)?;
    Ok(SqlitePathAnchor { file, identity })
}

pub(super) fn create_sqlite_stage(path: &Path) -> AppResult<SqlitePathAnchor> {
    let file = open_sqlite_anchor(path, true, true)?;
    let identity = file_identity(&file)?;
    file.sync_all()?;
    super::platform::sync_parent(path)?;
    Ok(SqlitePathAnchor { file, identity })
}

#[cfg(windows)]
pub(super) fn delete_file_with_identity(
    path: &Path,
    expected: &NativeFileIdentity,
) -> AppResult<()> {
    use std::os::windows::{fs::OpenOptionsExt, io::AsRawHandle};
    use windows_sys::Win32::{
        Foundation::GENERIC_READ,
        Storage::FileSystem::{
            DELETE, FILE_DISPOSITION_INFO, FILE_FLAG_OPEN_REPARSE_POINT, FILE_SHARE_READ,
            FILE_SHARE_WRITE, FileDispositionInfo, SetFileInformationByHandle,
        },
    };

    super::platform::reject_path_alias(path)?;
    let mut options = OpenOptions::new();
    options
        .access_mode(GENERIC_READ | DELETE)
        .share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    let file = options.open(path)?;
    if &file_identity(&file)? != expected {
        return Err(AppError::InvalidRequest(
            "owned SQLite stage changed before identity-bound deletion".into(),
        ));
    }
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: the handle includes DELETE access and the input is correctly sized.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle() as _,
            FileDispositionInfo,
            std::ptr::addr_of!(disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    drop(file);
    super::platform::sync_parent(path)
}

#[cfg(not(windows))]
pub(super) fn delete_file_with_identity(
    _path: &Path,
    _expected: &NativeFileIdentity,
) -> AppResult<()> {
    Err(AppError::InvalidRequest(
        "identity-bound SQLite deletion is not admitted on this platform".into(),
    ))
}

#[cfg(windows)]
fn open_sqlite_anchor(path: &Path, writable: bool, create_new: bool) -> AppResult<File> {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::{FILE_SHARE_READ, FILE_SHARE_WRITE};

    super::platform::reject_path_alias(path)?;
    #[cfg(test)]
    run_before_anchor_open(path);
    let mut options = OpenOptions::new();
    options.read(true).write(writable).create_new(create_new);
    // SQLite may reopen for reads/writes, but withholding delete sharing pins
    // this directory entry to the native identity held by the anchor.
    options.share_mode(FILE_SHARE_READ | FILE_SHARE_WRITE);
    options.custom_flags(windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT);
    Ok(options.open(path)?)
}

#[cfg(not(any(windows, unix)))]
fn open_sqlite_anchor(_path: &Path, _writable: bool, _create_new: bool) -> AppResult<File> {
    Err(AppError::InvalidRequest(
        "file-backed schema migration is not admitted on this platform".into(),
    ))
}

#[cfg(unix)]
fn open_sqlite_anchor(path: &Path, writable: bool, create_new: bool) -> AppResult<File> {
    use std::os::unix::fs::OpenOptionsExt;

    super::platform::reject_path_alias(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(writable).create_new(create_new);
    #[cfg(target_os = "linux")]
    const O_NOFOLLOW: i32 = 0x0002_0000;
    #[cfg(target_os = "macos")]
    const O_NOFOLLOW: i32 = 0x0000_0100;
    options.custom_flags(O_NOFOLLOW);
    Ok(options.open(path)?)
}

#[cfg(windows)]
fn file_identity(file: &File) -> AppResult<NativeFileIdentity> {
    use std::os::windows::io::AsRawHandle;
    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, GetFileInformationByHandle,
    };

    let mut information = std::mem::MaybeUninit::<BY_HANDLE_FILE_INFORMATION>::zeroed();
    // SAFETY: the anchor owns the live handle and output is correctly sized.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle() as _, information.as_mut_ptr()) }
        == 0
    {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: the successful call initialized the output structure.
    let information = unsafe { information.assume_init() };
    #[cfg(test)]
    if FORCE_NEXT_REPARSE.with(|forced| forced.replace(false)) {
        return Err(AppError::InvalidRequest(
            "SQLite migration handle resolves to a reparse point".into(),
        ));
    }
    identity_from_information(&information)
}

#[cfg(windows)]
fn identity_from_information(
    information: &windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
) -> AppResult<NativeFileIdentity> {
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;

    if information.dwFileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(AppError::InvalidRequest(
            "SQLite migration handle resolves to a reparse point".into(),
        ));
    }
    Ok(NativeFileIdentity {
        volume_serial: information.dwVolumeSerialNumber,
        file_index: (u64::from(information.nFileIndexHigh) << 32)
            | u64::from(information.nFileIndexLow),
    })
}

#[cfg(all(windows, test))]
type BeforeAnchorOpen = Option<Box<dyn FnOnce(&Path)>>;

#[cfg(all(windows, test))]
thread_local! {
    static BEFORE_ANCHOR_OPEN: std::cell::RefCell<BeforeAnchorOpen> =
        std::cell::RefCell::new(None);
    static FORCE_NEXT_REPARSE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

#[cfg(all(windows, test))]
fn run_before_anchor_open(path: &Path) {
    BEFORE_ANCHOR_OPEN.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook(path);
        }
    });
}

#[cfg(all(windows, test))]
pub(super) fn inject_anchor_open_reparse<F>(hook: F)
where
    F: FnOnce(&Path) + 'static,
{
    BEFORE_ANCHOR_OPEN.with(|slot| {
        assert!(slot.borrow().is_none(), "anchor-open test hook already set");
        *slot.borrow_mut() = Some(Box::new(hook));
    });
    FORCE_NEXT_REPARSE.with(|forced| forced.set(true));
}

#[cfg(unix)]
fn file_identity(file: &File) -> AppResult<NativeFileIdentity> {
    use std::os::unix::fs::MetadataExt;

    let metadata = file.metadata()?;
    if !metadata.file_type().is_file() {
        return Err(AppError::InvalidRequest(
            "SQLite migration anchor is not a regular file".into(),
        ));
    }
    let volume_serial = u32::try_from(metadata.dev()).map_err(|_| {
        AppError::InvalidRequest(
            "SQLite migration volume identity exceeds the admitted range".into(),
        )
    })?;
    Ok(NativeFileIdentity {
        volume_serial,
        file_index: metadata.ino(),
    })
}

#[cfg(not(any(windows, unix)))]
fn file_identity(_file: &File) -> AppResult<NativeFileIdentity> {
    Err(AppError::InvalidRequest(
        "native SQLite file identity is not admitted on this platform".into(),
    ))
}
