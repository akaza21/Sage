use std::{
    fs::{self, File, OpenOptions},
    path::Path,
};

use sage_core::error::{AppError, AppResult};

pub fn reject_path_alias(path: &Path) -> AppResult<()> {
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

pub fn sync_parent(path: &Path) -> AppResult<()> {
    #[cfg(windows)]
    {
        let _ = path;
        // Windows has no portable directory-fsync primitive. Publication uses
        // MoveFileExW(MOVEFILE_WRITE_THROUGH); cleanup is restart-idempotent.
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

pub fn remove_synced(path: &Path) -> AppResult<()> {
    match fs::remove_file(path) {
        Ok(()) => sync_parent(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

pub fn open_read_no_follow(path: &Path) -> AppResult<File> {
    reject_path_alias(path)?;
    let mut options = OpenOptions::new();
    options.read(true);
    configure_no_follow(&mut options);
    Ok(options.open(path)?)
}

#[cfg(windows)]
pub fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::windows::fs::OpenOptionsExt;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
}

#[cfg(target_os = "linux")]
pub fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOFOLLOW: i32 = 0x0002_0000;
    options.custom_flags(O_NOFOLLOW);
}

#[cfg(target_os = "macos")]
pub fn configure_no_follow(options: &mut OpenOptions) {
    use std::os::unix::fs::OpenOptionsExt;
    const O_NOFOLLOW: i32 = 0x0000_0100;
    options.custom_flags(O_NOFOLLOW);
}

#[cfg(not(any(windows, target_os = "linux", target_os = "macos")))]
pub fn configure_no_follow(_options: &mut OpenOptions) {}

#[cfg(unix)]
fn open_directory(path: &Path) -> AppResult<File> {
    Ok(File::open(path)?)
}
