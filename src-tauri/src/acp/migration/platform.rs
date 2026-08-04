use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use crate::error::{AppError, AppResult};

pub(super) use super::platform_anchor::{
    NativeFileIdentity, SqlitePathAnchor, anchor_existing_sqlite, create_sqlite_stage,
    delete_file_with_identity,
};

pub(super) struct MigrationLock {
    file: File,
}

impl MigrationLock {
    pub(super) fn acquire(parent: &Path) -> AppResult<Self> {
        let path = parent.join("sage.migration-v5.lock");
        reject_path_alias(&path)?;
        let file = open_lock_file(&path)?;
        lock_exclusive(&file)?;
        Ok(Self { file })
    }
}

pub(super) fn reject_path_alias(path: &Path) -> AppResult<()> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    #[cfg(unix)]
    let alias = metadata.file_type().is_symlink();
    #[cfg(windows)]
    let alias = {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0000_0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    };
    if alias {
        return Err(AppError::InvalidRequest(
            "migration paths must not be symbolic links or reparse aliases".into(),
        ));
    }
    Ok(())
}

impl Drop for MigrationLock {
    fn drop(&mut self) {
        unlock(&self.file);
    }
}

pub(super) fn require_free_bytes(parent: &Path, required: u64) -> AppResult<()> {
    #[cfg(test)]
    {
        let _ = parent;
        validate_available_bytes(
            super::test_failpoint::available_bytes().unwrap_or(u64::MAX),
            required,
        )
    }
    #[cfg(not(test))]
    let available = available_bytes(parent)?;
    #[cfg(not(test))]
    validate_available_bytes(available, required)
}

pub(super) fn validate_available_bytes(available: u64, required: u64) -> AppResult<()> {
    if available < required {
        return Err(AppError::ResourceLimit(format!(
            "migration requires {required} free bytes but only {available} are available"
        )));
    }
    Ok(())
}

pub(super) fn sync_file(path: &Path) -> AppResult<()> {
    reject_path_alias(path)?;
    let mut options = OpenOptions::new();
    options.read(true).write(true);
    configure_no_follow(&mut options);
    options.open(path)?.sync_all()?;
    Ok(())
}

pub(super) fn sync_parent(path: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        let _ = path;
        // Windows has no portable directory-fsync primitive. Publication uses
        // MoveFileExW(MOVEFILE_WRITE_THROUGH) below; cleanup is restart-idempotent.
        Ok(())
    }
    #[cfg(unix)]
    {
        let parent = path.parent().ok_or_else(|| {
            AppError::InvalidRequest("migration artifact has no parent directory".into())
        })?;
        open_directory(parent)?.sync_all()?;
        Ok(())
    }
}

pub(super) fn rename_synced(from: &Path, to: &Path) -> AppResult<()> {
    reject_path_alias(from)?;
    reject_path_alias(to)?;
    if to.exists() {
        return Err(AppError::InvalidRequest(
            "migration publication target already exists".into(),
        ));
    }
    #[cfg(unix)]
    fs::rename(from, to)?;
    #[cfg(windows)]
    rename_write_through(from, to)?;
    sync_parent(to)
}

pub(super) fn remove_synced(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn copy_bounded_create_new(source: &Path, target: &Path, cap: u64) -> AppResult<()> {
    reject_path_alias(source)?;
    reject_path_alias(target)?;
    let source_metadata = fs::metadata(source)?;
    if source_metadata.len() > cap {
        return Err(AppError::ResourceLimit(
            "probe source exceeds its physical byte limit".into(),
        ));
    }
    let mut source_options = OpenOptions::new();
    source_options.read(true);
    configure_no_follow(&mut source_options);
    let mut target_options = OpenOptions::new();
    target_options.write(true).create_new(true);
    configure_no_follow(&mut target_options);
    let mut input = source_options.open(source)?;
    let mut output = target_options.open(target)?;
    let mut total = 0_u64;
    let mut buffer = [0_u8; 1_048_576];
    loop {
        let read = input.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        total = total.saturating_add(read as u64);
        if total > cap {
            return Err(AppError::ResourceLimit(
                "probe source grew beyond its physical byte limit".into(),
            ));
        }
        output.write_all(&buffer[..read])?;
    }
    if total != source_metadata.len() {
        return Err(AppError::InvalidRequest(
            "probe source changed length while it was copied".into(),
        ));
    }
    output.sync_all()?;
    drop(output);
    sync_parent(target)
}

pub(super) fn write_bounded_create_new(path: &Path, bytes: &[u8], cap: u64) -> AppResult<()> {
    reject_path_alias(path)?;
    if bytes.len() as u64 > cap {
        return Err(AppError::ResourceLimit(
            "migration artifact exceeds its byte limit".into(),
        ));
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    configure_no_follow(&mut options);
    let mut file = options.open(path)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    drop(file);
    sync_parent(path)
}

pub(super) fn open_read_no_follow(path: &Path) -> AppResult<File> {
    reject_path_alias(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    Ok(options.open(path)?)
}

pub(super) fn file_is_prefix_of(source: &Path, candidate: &Path, cap: u64) -> AppResult<bool> {
    reject_path_alias(source)?;
    reject_path_alias(candidate)?;
    let source_bytes = fs::metadata(source)?.len();
    let candidate_bytes = fs::metadata(candidate)?.len();
    if source_bytes > cap || candidate_bytes > source_bytes {
        return Ok(false);
    }
    let mut source_options = OpenOptions::new();
    source_options.read(true);
    configure_no_follow(&mut source_options);
    let mut candidate_options = OpenOptions::new();
    candidate_options.read(true);
    configure_no_follow(&mut candidate_options);
    let mut source = source_options.open(source)?;
    let mut candidate = candidate_options.open(candidate)?;
    let mut source_buffer = vec![0_u8; 1_048_576];
    let mut candidate_buffer = vec![0_u8; 1_048_576];
    let mut remaining = candidate_bytes;
    while remaining > 0 {
        let wanted = usize::try_from(remaining.min(candidate_buffer.len() as u64))
            .expect("bounded buffer length fits usize");
        candidate.read_exact(&mut candidate_buffer[..wanted])?;
        source.read_exact(&mut source_buffer[..wanted])?;
        if candidate_buffer[..wanted] != source_buffer[..wanted] {
            return Ok(false);
        }
        remaining -= wanted as u64;
    }
    Ok(true)
}

#[cfg(windows)]
fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(target_os = "linux")]
fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOFOLLOW: i32 = 0x0002_0000;
    options.custom_flags(O_NOFOLLOW);
}

#[cfg(target_os = "macos")]
fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOFOLLOW: i32 = 0x0000_0100;
    options.custom_flags(O_NOFOLLOW);
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
fn configure_no_follow(_options: &mut OpenOptions) {}

fn open_lock_file(path: &Path) -> AppResult<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    configure_no_follow(&mut options);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        options.share_mode(0);
    }
    Ok(options.open(path)?)
}

#[cfg(unix)]
fn lock_exclusive(file: &File) -> AppResult<()> {
    use std::os::fd::AsRawFd;
    const LOCK_EXCLUSIVE: i32 = 2;
    const LOCK_NONBLOCKING: i32 = 4;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // SAFETY: flock receives a live descriptor owned by `file`; the descriptor
    // remains alive in MigrationLock until the matching unlock/drop.
    let result = unsafe { flock(file.as_raw_fd(), LOCK_EXCLUSIVE | LOCK_NONBLOCKING) };
    if result != 0 {
        return Err(AppError::InvalidRequest(
            "another Sage process owns schema migration".into(),
        ));
    }
    Ok(())
}

#[cfg(unix)]
fn unlock(file: &File) {
    use std::os::fd::AsRawFd;
    const LOCK_UN: i32 = 8;
    unsafe extern "C" {
        fn flock(fd: i32, operation: i32) -> i32;
    }
    // SAFETY: this only releases the advisory lock held by this live file.
    let _ = unsafe { flock(file.as_raw_fd(), LOCK_UN) };
}

#[cfg(windows)]
fn lock_exclusive(_file: &File) -> AppResult<()> {
    // OpenOptionsExt::share_mode(0) makes the live handle the exclusive
    // process owner. A competing open fails before any migration action.
    Ok(())
}

#[cfg(windows)]
fn unlock(_file: &File) {}

#[cfg(unix)]
fn open_directory(path: &Path) -> AppResult<File> {
    Ok(File::open(path)?)
}

#[cfg(windows)]
fn rename_write_through(from: &Path, to: &Path) -> AppResult<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::{MOVEFILE_WRITE_THROUGH, MoveFileExW};
    if to.exists() {
        return Err(AppError::InvalidRequest(
            "migration publication target already exists".into(),
        ));
    }
    let mut from = from.as_os_str().encode_wide().collect::<Vec<_>>();
    let mut to = to.as_os_str().encode_wide().collect::<Vec<_>>();
    from.push(0);
    to.push(0);
    // SAFETY: both paths are NUL-terminated UTF-16 buffers valid for the call.
    if unsafe { MoveFileExW(from.as_ptr(), to.as_ptr(), MOVEFILE_WRITE_THROUGH) } == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(())
}

#[cfg(all(target_os = "linux", not(test)))]
fn available_bytes(path: &Path) -> AppResult<u64> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    #[repr(C)]
    struct Statvfs {
        block_size: u64,
        fragment_size: u64,
        blocks: u64,
        blocks_free: u64,
        blocks_available: u64,
        files: u64,
        files_free: u64,
        files_available: u64,
        filesystem_id: u64,
        mount_flags: u64,
        name_max: u64,
        spare: [i32; 6],
    }
    unsafe extern "C" {
        fn statvfs(path: *const i8, output: *mut Statvfs) -> i32;
    }
    let path = CString::new(path.as_os_str().as_bytes())
        .map_err(|_| AppError::InvalidRequest("migration directory contains a NUL byte".into()))?;
    let mut output = std::mem::MaybeUninit::<Statvfs>::uninit();
    // SAFETY: `path` is NUL-terminated and output points to writable storage of
    // the Linux statvfs layout for the duration of the call.
    if unsafe { statvfs(path.as_ptr(), output.as_mut_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: statvfs returned success and initialized the output structure.
    let output = unsafe { output.assume_init() };
    let allocation_size = if output.fragment_size != 0 {
        output.fragment_size
    } else {
        output.block_size
    };
    Ok(output.blocks_available.saturating_mul(allocation_size))
}

#[cfg(all(windows, not(test)))]
fn available_bytes(path: &Path) -> AppResult<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let mut path = path.as_os_str().encode_wide().collect::<Vec<_>>();
    path.push(0);
    let mut available = 0_u64;
    // SAFETY: `path` is a NUL-terminated UTF-16 buffer and `available` is a
    // valid output pointer; the optional totals are intentionally omitted.
    let result = unsafe {
        GetDiskFreeSpaceExW(
            path.as_ptr(),
            &mut available,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    if result == 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    Ok(available)
}

#[cfg(all(not(any(target_os = "linux", windows)), not(test)))]
fn available_bytes(_path: &Path) -> AppResult<u64> {
    Err(AppError::InvalidRequest(
        "migration free-space probing is not yet admitted on this platform".into(),
    ))
}
